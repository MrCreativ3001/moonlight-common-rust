use sans_io_time::Instant as SInstant;
use std::{io, net::SocketAddr, time::Instant};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpStream, UdpSocket},
    time::sleep_until,
};

use crate::stream::proto::runtime::{self, AsyncTcpStream, AsyncUdpSocket, Runtime};

pub type MoonlightStreamError = runtime::MoonlightStreamError<io::Error>;

impl From<io::Error> for MoonlightStreamError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

#[derive(Debug, Clone)]
pub struct TokioRuntime {
    base_time: Instant,
}

impl TokioRuntime {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self {
            base_time: Instant::now(),
        }
    }
}

impl Runtime for TokioRuntime {
    type Error = io::Error;

    fn now(&self) -> SInstant {
        SInstant::from_std(self.base_time)
    }

    async fn sleep_until(&self, deadline: SInstant) {
        sleep_until(deadline.to_std(self.base_time).into()).await
    }

    type TcpStream = TcpStream;

    async fn connect_tcp_stream(&self, addr: SocketAddr) -> Result<Self::TcpStream, Self::Error> {
        let stream = TcpStream::connect(addr).await?;

        Ok(stream)
    }

    type UdpSocket = UdpSocket;

    async fn bind_udp_socket(&self) -> Result<Self::UdpSocket, Self::Error> {
        let socket = UdpSocket::bind("0.0.0.0:0").await?;

        Ok(socket)
    }
}

impl AsyncTcpStream<io::Error> for TcpStream {
    async fn write(&mut self, buffer: &[u8]) -> Result<usize, io::Error> {
        AsyncWriteExt::write(self, buffer).await
    }

    async fn read(&mut self, buffer: &mut [u8]) -> Result<usize, io::Error> {
        AsyncReadExt::read(self, buffer).await
    }
}

impl AsyncUdpSocket<io::Error> for UdpSocket {
    async fn recv(&self, buffer: &mut [u8]) -> Result<(SocketAddr, usize), io::Error> {
        let (len, addr) = UdpSocket::recv_from(self, buffer).await?;

        Ok((addr, len))
    }

    async fn writable(&self) -> Result<(), io::Error> {
        UdpSocket::writable(self).await
    }

    fn try_send(&self, addr: SocketAddr, buffer: &[u8]) -> Result<bool, io::Error> {
        match UdpSocket::try_send_to(self, buffer, addr) {
            Ok(_) => Ok(true),
            Err(err) if err.kind() == io::ErrorKind::WouldBlock => Ok(false),
            Err(err) => Err(err),
        }
    }
}
