use std::{net::SocketAddr, time::Duration};

use futures::{
    FutureExt,
    future::{Either, pending},
    select,
};
use sans_io_time::Instant;

// TODO: UdpStream send and receive should have an address
pub trait UdpStream: Send + Sync {
    type Error;

    type Event;

    fn pending_send(&self) -> Option<(SocketAddr, &[u8])>;
    fn consume_send(&mut self);

    fn poll_timeout(&self) -> Option<Instant>;

    fn poll_event(&mut self) -> Option<Self::Event>;

    fn handle_receive(
        &mut self,
        now: Instant,
        addr: SocketAddr,
        data: &[u8],
    ) -> Result<(), Self::Error>;

    fn handle_timeout(&mut self, now: Instant) -> Result<(), Self::Error>;
}

pub trait Runtime {
    type Socket: AsyncUdpSocket;

    fn now(&self) -> Instant;

    fn sleep_until(&self, deadline: Instant) -> impl Future<Output = ()>;

    fn bind_udp_socket(
        &self,
    ) -> impl Future<Output = Result<Self::Socket, <Self::Socket as AsyncUdpSocket>::Error>>;
}

pub trait AsyncUdpSocket: Sized {
    type Error;

    /// # Cancel Safety
    /// This function is cancel safe.
    /// If it is cancelled no packet was consumed.
    fn recv(
        &self,
        buffer: &mut [u8],
    ) -> impl Future<Output = Result<(SocketAddr, usize), Self::Error>>;

    /// # Cancel Safety
    /// This function is cancel safe.
    fn writable(&self) -> impl Future<Output = Result<(), Self::Error>>;

    fn try_send(&self, addr: SocketAddr, buffer: &[u8]) -> Result<bool, Self::Error>;
}

pub struct AsyncUdpDriver<Rt, Stream>
where
    Rt: Runtime,
{
    runtime: Rt,
    socket: Rt::Socket,
    stream: Stream,
    read_buffer: Vec<u8>,
}

impl<Rt, Stream> AsyncUdpDriver<Rt, Stream>
where
    Rt: Runtime,
    Stream: UdpStream,
{
    pub async fn connect(
        runtime: Rt,
        stream: Stream,
    ) -> Result<Self, <Rt::Socket as AsyncUdpSocket>::Error> {
        let socket = runtime.bind_udp_socket().await?;

        Ok(Self {
            runtime,
            socket,
            stream,
            read_buffer: vec![0; 4096],
        })
    }

    pub fn stream_mut(&mut self) -> &mut Stream {
        &mut self.stream
    }
    pub fn stream(&self) -> &Stream {
        &self.stream
    }

    /// # Cancel Safety
    /// This function is cancel safe.
    /// This means that no event has been consumed and no state is lost when this function is cancelled.
    pub async fn poll_event<Error>(&mut self) -> Result<Stream::Event, Error>
    where
        Error: From<<Rt::Socket as AsyncUdpSocket>::Error> + From<Stream::Error>,
    {
        loop {
            if let Some(event) = self.stream.poll_event() {
                return Ok(event);
            }

            let timeout = self
                .stream
                .poll_timeout()
                .unwrap_or(self.runtime.now() + Duration::from_secs(1));
            let timeout = self.runtime.sleep_until(timeout);

            let writable = self
                .stream
                .pending_send()
                .map(|_| Either::Left(self.socket.writable()))
                .unwrap_or(Either::Right(pending()));

            select! {
                result = writable.fuse() => {
                    result?;

                    while let Some((addr,pending_send)) = self.stream.pending_send() {
                        if !self.socket.try_send(addr, pending_send)? {
                            break;
                        }
                        self.stream.consume_send();
                    }
                },
                result = self.socket.recv(&mut self.read_buffer).fuse() => {
                    let (addr, len) = result?;

                    self.stream.handle_receive(self.runtime.now(), addr, &self.read_buffer[0..len])?;
                }
                _ = timeout.fuse() => {
                    self.stream.handle_timeout(self.runtime.now())?;
                }
            }
        }
    }
}
