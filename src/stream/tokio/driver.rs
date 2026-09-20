use std::{
    io,
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};

use pin_project_lite::pin_project;
use sans_io_time::Instant as SansInstant;
use tokio::{
    io::ReadBuf,
    net::UdpSocket,
    time::{Instant, Sleep, sleep_until},
};

use crate::stream::{
    proto::runtime::UdpStream, sockets::new_udp_socket, tokio::MoonlightStreamError,
};

pub struct StreamDriver<Stream> {
    base_time: Instant,
    inner: Stream,
    socket: UdpSocket,
    recv_buffer: Vec<u8>,
}

impl<Stream> StreamDriver<Stream>
where
    Stream: UdpStream,
{
    pub async fn new(base_time: Instant, stream: Stream) -> Result<Self, MoonlightStreamError> {
        let socket = new_udp_socket(false, stream.recv_buffer_hint())?;

        socket.set_nonblocking(true)?;
        let socket = UdpSocket::from_std(socket)?;

        Ok(Self {
            base_time,
            inner: stream,
            socket,
            recv_buffer: vec![0; 4096],
        })
    }

    pub fn drive(&mut self) -> DriveFuture<'_, Stream> {
        let deadline = self
            .inner
            .poll_timeout()
            .map(|x| x.to_std(self.base_time.into_std()).into())
            .unwrap_or_else(|| Instant::now() + Duration::from_secs(1));

        DriveFuture {
            driver: self,
            old_deadline: deadline,
            sleep: sleep_until(deadline),
        }
    }

    pub fn stream(&self) -> &Stream {
        &self.inner
    }
    pub fn stream_mut(&mut self) -> &mut Stream {
        &mut self.inner
    }
}

pin_project! {
    pub struct DriveFuture<'a, Stream> {
        driver: &'a mut StreamDriver<Stream>,
        old_deadline: Instant,
        #[pin]
        sleep: Sleep,
    }
}

impl<'a, Stream> Future for DriveFuture<'a, Stream>
where
    Stream: UdpStream,
    MoonlightStreamError: From<Stream::Error>,
{
    type Output = Result<Stream::Event, MoonlightStreamError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut this = self.project();

        loop {
            // -- Write
            #[allow(clippy::collapsible_if)]
            if let Some((mut addr, mut buffer)) = this.driver.inner.pending_send() {
                if this.driver.socket.poll_send_ready(cx).is_ready() {
                    loop {
                        // Try to write
                        match this.driver.socket.try_send_to(buffer, addr) {
                            Ok(_) => {
                                // remove packet
                                this.driver.inner.consume_send();
                            }
                            Err(err) if matches!(err.kind(), io::ErrorKind::WouldBlock) => {
                                // We cannot send anymore
                                break;
                            }
                            Err(err) => return Poll::Ready(Err(err.into())),
                        }

                        if let Some((new_addr, new_buffer)) = this.driver.inner.pending_send() {
                            // Try to get next packet and write
                            addr = new_addr;
                            buffer = new_buffer;
                        } else {
                            // No next packet
                            break;
                        }
                    }
                }
            }

            // -- Read
            let mut received = false;
            loop {
                let mut recv_buffer = ReadBuf::new(&mut this.driver.recv_buffer);

                match this.driver.socket.poll_recv_from(cx, &mut recv_buffer) {
                    Poll::Ready(Ok(addr)) => {
                        received = true;

                        this.driver.inner.handle_receive(
                            SansInstant::from_std(this.driver.base_time.into_std()),
                            addr,
                            recv_buffer.filled(),
                        )?;
                        recv_buffer.clear();
                    }
                    Poll::Ready(Err(err)) => return Poll::Ready(Err(err.into())),
                    Poll::Pending => break,
                }
            }
            if received {
                // If data was received, we might have a new send
                continue;
            }

            // -- Timeout
            let deadline = this
                .driver
                .inner
                .poll_timeout()
                .map(|x| x.to_std(this.driver.base_time.into_std()).into());

            if let Some(deadline) = deadline {
                // Set new timeout if needed
                if *this.old_deadline != deadline {
                    *this.old_deadline = deadline;
                    this.sleep.as_mut().reset(deadline);
                }

                // Poll Timeout
                if this.sleep.as_mut().poll(cx).is_ready() {
                    this.driver
                        .inner
                        .handle_timeout(SansInstant::from_std(this.driver.base_time.into_std()))?;
                    continue;
                }
            }

            break;
        }

        // -- Event
        if let Some(event) = this.driver.inner.poll_event() {
            return Poll::Ready(Ok(event));
        }

        Poll::Pending
    }
}

#[cfg(test)]
mod tests {
    use sans_io_time::Instant as SansInstant;
    use std::{collections::VecDeque, convert::Infallible, net::SocketAddr, time::Duration};
    use tokio::{
        net::UdpSocket,
        select, spawn,
        time::{Instant, sleep},
    };

    use crate::stream::{proto::runtime::UdpStream, tokio::driver::StreamDriver};

    enum TestEvent {
        Timeout(SansInstant),
        Receive {
            now: SansInstant,
            #[allow(unused)]
            addr: SocketAddr,
            data: Vec<u8>,
        },
    }
    #[derive(Default)]
    struct TestStream {
        send_list: VecDeque<(SocketAddr, Vec<u8>)>,
        event_list: VecDeque<TestEvent>,
        timeout: Option<SansInstant>,
    }
    impl UdpStream for TestStream {
        type Error = Infallible;

        type Event = TestEvent;

        fn consume_send(&mut self) {
            self.send_list.pop_front();
        }
        fn pending_send(&self) -> Option<(SocketAddr, &[u8])> {
            self.send_list
                .front()
                .map(|(addr, data)| (*addr, data.as_slice()))
        }

        fn poll_event(&mut self) -> Option<Self::Event> {
            self.event_list.pop_front()
        }
        fn poll_timeout(&self) -> Option<SansInstant> {
            self.timeout
        }

        fn handle_receive(
            &mut self,
            now: SansInstant,
            addr: SocketAddr,
            data: &[u8],
        ) -> Result<(), Self::Error> {
            self.event_list.push_back(TestEvent::Receive {
                now,
                addr,
                data: data.to_vec(),
            });
            Ok(())
        }
        fn handle_timeout(&mut self, now: SansInstant) -> Result<(), Self::Error> {
            self.event_list.push_back(TestEvent::Timeout(now));
            Ok(())
        }
    }

    #[tokio::test]
    async fn handle_timeout() {
        let base_time = Instant::now();

        let duration = Duration::from_millis(100);

        let stream = TestStream {
            timeout: Some(SansInstant::ZERO + duration),
            ..Default::default()
        };
        let mut driver = StreamDriver::new(base_time, stream).await.unwrap();

        select! {
            _ = sleep(Duration::from_millis(200)) => {
                panic!("timeout didn't happen");
            }
            result = driver.drive() => {
                let TestEvent::Timeout(timeout) = result.unwrap() else {
                    panic!("invalid event");
                };

                let timeout_end = Instant::from_std(timeout.to_std(base_time.into_std()));
                let diff = if timeout_end > (base_time + duration) {
                    timeout_end - (base_time + duration)
                } else {
                    (base_time + duration) - timeout_end
                };

                assert!(diff <= Duration::from_millis(10), "driver slept too long or short");
            }
        }
    }

    #[tokio::test]
    async fn handle_receive() {
        let socket = UdpSocket::bind("0.0.0.0:0").await.unwrap();

        let base_time = Instant::now();
        let test_data = &[0, 1, 2, 3];
        let packet_duration = Duration::from_millis(100);

        let stream = TestStream {
            timeout: Some(SansInstant::ZERO + Duration::from_millis(200)),
            ..Default::default()
        };
        let mut driver = StreamDriver::new(base_time, stream).await.unwrap();

        let driver_addr = driver.socket.local_addr().unwrap();
        println!("driver_addr = {driver_addr}");

        spawn(async move {
            sleep(packet_duration).await;

            socket.send_to(test_data, driver_addr).await.unwrap();
        });

        select! {
            result = driver.drive() => {
                let TestEvent::Receive{
                    now: timeout,
                    addr: _,
                    data,
                } = result.unwrap() else {
                    panic!("invalid event");
                };

                assert_eq!(data, test_data);

                let timeout_end = Instant::from_std(timeout.to_std(base_time.into_std()));
                let diff = if timeout_end > (base_time + packet_duration) {
                    timeout_end - (base_time + packet_duration)
                } else {
                    (base_time + packet_duration) - timeout_end
                };

                assert!(diff <= Duration::from_millis(15), "driver slept too long or short");
            }
        }
    }

    #[tokio::test]
    async fn handle_mixed() {
        let socket = UdpSocket::bind("0.0.0.0:0").await.unwrap();

        let base_time = Instant::now();
        let test_data = &[0, 1, 2, 3];
        let timeout_duration = Duration::from_millis(200);
        let packet_delay = Duration::from_millis(100);

        let stream = TestStream {
            timeout: Some(SansInstant::ZERO + timeout_duration),
            ..Default::default()
        };
        let mut driver = StreamDriver::new(base_time, stream).await.unwrap();

        // 1. Wait 100ms, then send a packet to the driver
        sleep(packet_delay).await;

        let mut driver_addr = driver.socket.local_addr().unwrap();
        if driver_addr.ip().is_unspecified() {
            driver_addr.set_ip(std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST));
        }
        socket.send_to(test_data, driver_addr).await.unwrap();

        // 2. Drive the first time: Expecting the packet to be received
        select! {
            _ = sleep(Duration::from_millis(150)) => {
                panic!("packet was not received in time");
            }
            result = driver.drive() => {
                let TestEvent::Receive { now, addr: _, data } = result.unwrap() else {
                    panic!("expected Receive event, got something else");
                };
                assert_eq!(data, test_data);

                let receive_time = Instant::from_std(now.to_std(base_time.into_std()));
                let diff = if receive_time > (base_time + packet_delay) {
                    receive_time - (base_time + packet_delay)
                } else {
                    (base_time + packet_delay) - receive_time
                };
                assert!(diff <= Duration::from_millis(15), "packet receive timing off");
            }
        }

        // 3. Drive the second time: Expecting the timeout to fire for the remaining ~100ms
        select! {
            _ = sleep(Duration::from_millis(200)) => {
                panic!("timeout didn't happen after packet processing");
            }
            result = driver.drive() => {
                let TestEvent::Timeout(timeout) = result.unwrap() else {
                    panic!("expected Timeout event after packet processing");
                };

                let timeout_end = Instant::from_std(timeout.to_std(base_time.into_std()));
                let diff = if timeout_end > (base_time + timeout_duration) {
                    timeout_end - (base_time + timeout_duration)
                } else {
                    (base_time + timeout_duration) - timeout_end
                };
                assert!(diff <= Duration::from_millis(15), "driver slept too long or short for timeout");
            }
        }
    }
}
