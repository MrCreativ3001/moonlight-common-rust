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
use tracing::warn;

use crate::{
    error::Error,
    stream::{
        proto::runtime::{Receive, Transmit, UdpStream},
        sockets::new_udp_socket,
    },
};

pub struct StreamDriver<Stream> {
    pub(crate) base_time: Instant,
    pub(crate) inner: Stream,
    pub(crate) socket: UdpSocket,
    pub(crate) transmit: Option<Transmit>,
    pub(crate) recv_buffer: Vec<u8>,
}

impl<Stream> StreamDriver<Stream>
where
    Stream: UdpStream,
{
    pub async fn new(base_time: Instant, stream: Stream) -> Result<Self, Error> {
        let socket = new_udp_socket(false, stream.recv_buffer_hint())?;

        socket.set_nonblocking(true)?;
        let socket = UdpSocket::from_std(socket)?;

        Ok(Self {
            base_time,
            inner: stream,
            socket,
            transmit: None,
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

/// The maximum amount of time the drive loop is allowed to loop
const MAX_LOOP: usize = 50;

impl<'a, Stream> Future for DriveFuture<'a, Stream>
where
    Stream: UdpStream,
{
    type Output = Result<Stream::Event, Error>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut this = self.project();
        let mut loops = 0;

        loop {
            if loops > MAX_LOOP {
                warn!(
                    "tokio driver looped more than {MAX_LOOP} times. This hints at a performance issue or a bug!"
                );
                break;
            }
            loops += 1;

            // -- Write
            // Try to get next packet and write
            if this.driver.transmit.is_none()
                && let Some(new_transmit) = this.driver.inner.poll_transmit()
            {
                this.driver.transmit = Some(new_transmit);
            }

            // Try to write the available packet
            #[allow(clippy::collapsible_if)]
            if let Some(transmit) = this.driver.transmit.as_mut() {
                if this.driver.socket.poll_send_ready(cx).is_ready() {
                    loop {
                        // Try to write
                        match this
                            .driver
                            .socket
                            .try_send_to(&transmit.data, transmit.destination)
                        {
                            Ok(_) => {
                                // fallthrough
                            }
                            Err(err) if matches!(err.kind(), io::ErrorKind::WouldBlock) => {
                                // We cannot send anymore
                                break;
                            }
                            Err(err) => return Poll::Ready(Err(err.into())),
                        }

                        if let Some(new_transmit) = this.driver.inner.poll_transmit() {
                            // Try to get next packet and write
                            *transmit = new_transmit;
                        } else {
                            // No next packet, remove it
                            this.driver.transmit.take();
                            break;
                        }
                    }
                }
            }

            // Deliver queued events before another receive or timeout can fail
            if let Some(event) = this.driver.inner.poll_event() {
                return Poll::Ready(Ok(event));
            }

            // -- Read
            let mut recv_buffer = ReadBuf::new(&mut this.driver.recv_buffer);
            match this.driver.socket.poll_recv_from(cx, &mut recv_buffer) {
                Poll::Ready(Ok(addr)) => {
                    this.driver.inner.handle_receive(
                        SansInstant::from_std(this.driver.base_time.into_std()),
                        Receive {
                            source: addr,
                            data: recv_buffer.filled(),
                        },
                    )?;
                    // Flush any response and deliver events before reading again.
                    continue;
                }
                Poll::Ready(Err(err)) => return Poll::Ready(Err(err.into())),
                Poll::Pending => {}
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

        if let Some(event) = this.driver.inner.poll_event() {
            return Poll::Ready(Ok(event));
        }

        Poll::Pending
    }
}
