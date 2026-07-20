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

use crate::stream::{proto::runtime::UdpStream, tokio::MoonlightStreamError};

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
    pub async fn new(stream: Stream) -> Result<Self, MoonlightStreamError> {
        let socket = UdpSocket::bind("0.0.0.0:0").await?;

        Ok(Self {
            base_time: Instant::now(),
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
                        .handle_timeout(SansInstant::from_std(Instant::now().into_std()))?;
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
