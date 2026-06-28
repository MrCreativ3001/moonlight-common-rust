use std::net::SocketAddr;

use sans_io_time::Instant;

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
