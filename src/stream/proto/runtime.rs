use std::net::SocketAddr;

use bytes::Bytes;
use sans_io_time::Instant;

use crate::error::Error;

#[derive(Debug, PartialEq)]
pub struct Transmit {
    pub destination: SocketAddr,
    pub data: Bytes,
}

#[derive(Debug, PartialEq)]
pub struct Receive<'a> {
    pub source: SocketAddr,
    pub data: &'a [u8],
}

pub trait UdpStream: Send + Sync {
    type Event;

    fn poll_transmit(&mut self) -> Option<Transmit>;

    fn poll_timeout(&self) -> Option<Instant>;

    fn poll_event(&mut self) -> Option<Self::Event>;

    fn handle_receive(&mut self, now: Instant, receive: Receive) -> Result<(), Error>;

    fn handle_timeout(&mut self, now: Instant) -> Result<(), Error>;

    /// Allows for setting an os hint for the receive buffer size of the udp socket.
    fn recv_buffer_hint(&self) -> Option<usize> {
        None
    }
}

#[cfg(test)]
pub(crate) fn drive_sleep<Stream>(
    stream: &mut Stream,
    now: &mut Instant,
    timeout: std::time::Duration,
    expected_transmits: &[Transmit],
) -> Vec<Stream::Event>
where
    Stream: UdpStream,
{
    drive_until(stream, *now + timeout, now, expected_transmits)
}

#[cfg(test)]
pub(crate) fn drive_until<Stream>(
    stream: &mut Stream,
    until: Instant,
    now: &mut Instant,
    expected_transmits: &[Transmit],
) -> Vec<Stream::Event>
where
    Stream: UdpStream,
{
    let mut i = 0;

    let mut transmits = Vec::default();
    let mut events = Vec::default();

    loop {
        stream.handle_timeout(*now).unwrap();

        while let Some(transmit) = stream.poll_transmit() {
            transmits.push(transmit);
        }
        while let Some(event) = stream.poll_event() {
            events.push(event);
        }

        if *now >= until {
            break;
        }

        i += 1;
        if i > 100 {
            panic!("iterated {i} times");
        }

        let timeout = stream.poll_timeout().unwrap();
        assert!(timeout > *now);

        *now = until.min(timeout);
    }

    for expected_transmit in expected_transmits {
        assert!(
            transmits.contains(expected_transmit),
            "transmits, expected: {expected_transmit:?}, got: {transmits:?}"
        );
    }

    events
}
