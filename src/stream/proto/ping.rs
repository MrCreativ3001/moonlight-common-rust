use std::time::Duration;

use sans_io_time::Instant;

use smallvec::{SmallVec, smallvec};
use tracing::{Level, debug, instrument};

use crate::stream::proto::packet::{SunshinePing, SunshinePingPacket};

const PING_RETRY_TIMEOUT: Duration = Duration::from_millis(500);
const LEGACY_PING: &[u8] = &[0x50, 0x49, 0x4E, 0x47];

#[derive(Debug)]
pub struct PingSenderConfig {
    pub sunshine_ping: Option<SunshinePing>,
}

#[derive(Debug)]
pub struct PingSender {
    now: Instant,
    current_ping_send: Instant,
    config: PingSenderConfig,
    next_attempt: u32,
    current_ping_packet: SmallVec<[u8; SunshinePingPacket::SIZE]>,
}

impl PingSender {
    #[instrument(level = Level::DEBUG)]
    pub fn new(now: Instant, config: PingSenderConfig) -> Self {
        let mut this = Self {
            now,
            current_ping_send: now,
            config,
            next_attempt: 1,
            current_ping_packet: smallvec![],
        };
        this.write_packet(0);

        this
    }

    fn write_packet(&mut self, sequence_number: u32) {
        self.current_ping_packet.resize(SunshinePingPacket::SIZE, 0);
        let current_ping_packet = self
            .current_ping_packet
            .as_mut_array()
            .expect("array with ping packet size");

        let packet_len = if let Some(ping) = self.config.sunshine_ping.as_ref() {
            // Use Sunshine ping
            let packet = SunshinePingPacket {
                payload: ping.clone(),
                sequence_number,
            };

            packet.serialize(current_ping_packet);
            SunshinePingPacket::SIZE
        } else {
            // Just some magic bytes
            let ping = LEGACY_PING;

            current_ping_packet[0..ping.len()].copy_from_slice(ping);
            ping.len()
        };
        self.current_ping_packet.truncate(packet_len);

        let packet = &self.current_ping_packet[0..packet_len];
        debug!(packet = ?packet, "sending ping");
    }

    fn advance_packet(&mut self) {
        // Advance next ping send
        self.current_ping_send += PING_RETRY_TIMEOUT;

        // Overwrite current ping buffer with the new packet
        self.write_packet(self.next_attempt);

        // Advance attempt
        self.next_attempt += 1;
    }

    pub fn poll_timeout(&self) -> Option<Instant> {
        Some(self.current_ping_send)
    }

    pub fn pending_send(&self) -> Option<&[u8]> {
        if self.now < self.current_ping_send {
            return None;
        }

        Some(&self.current_ping_packet)
    }
    pub fn consume_send(&mut self) {
        self.advance_packet();
    }

    pub fn handle_timeout(&mut self, now: Instant) {
        self.now = now;

        // Check if we've reached the timeout
        if self.now < self.current_ping_send + PING_RETRY_TIMEOUT {
            return;
        }

        self.advance_packet();
    }
}

#[cfg(test)]
mod tests {
    use sans_io_time::Instant;

    use crate::stream::proto::{
        packet::{SunshinePing, SunshinePingPacket},
        ping::{LEGACY_PING, PING_RETRY_TIMEOUT, PingSender, PingSenderConfig},
    };

    #[test]
    fn ping_legacy() {
        let mut time = Instant::from_nanos(0);

        let mut sender = PingSender::new(
            time,
            PingSenderConfig {
                sunshine_ping: None,
            },
        );

        // check for first ping
        assert_eq!(sender.poll_timeout(), Some(time));
        assert_eq!(sender.pending_send(), Some(LEGACY_PING));

        // consume ping
        sender.consume_send();
        assert_eq!(sender.pending_send(), None);
        assert_eq!(sender.poll_timeout(), Some(time + PING_RETRY_TIMEOUT));

        // advance time only by half
        time += PING_RETRY_TIMEOUT / 2;
        sender.handle_timeout(time);

        // check for no ping
        assert_eq!(sender.poll_timeout(), Some(time + (PING_RETRY_TIMEOUT / 2)));
        assert_eq!(sender.pending_send(), None);

        // advance time by another half
        time += PING_RETRY_TIMEOUT / 2;
        sender.handle_timeout(time);

        // check for second ping
        assert_eq!(sender.poll_timeout(), Some(time));
        assert_eq!(sender.pending_send(), Some(LEGACY_PING));

        // consume ping
        sender.consume_send();
        assert_eq!(sender.pending_send(), None);
        assert_eq!(sender.poll_timeout(), Some(time + PING_RETRY_TIMEOUT));
    }

    fn sunshine_ping(ping: SunshinePing, sequence_number: u32) -> [u8; SunshinePingPacket::SIZE] {
        let packet = SunshinePingPacket {
            payload: ping,
            sequence_number,
        };

        let mut bytes = [0; _];
        packet.serialize(&mut bytes);

        bytes
    }

    #[test]
    fn ping_sunshine() {
        let mut time = Instant::from_nanos(0);
        let ping = SunshinePing([
            54, 53, 48, 69, 57, 67, 66, 52, 54, 51, 57, 65, 53, 54, 70, 70,
        ]);

        let mut sender = PingSender::new(
            time,
            PingSenderConfig {
                sunshine_ping: Some(ping.clone()),
            },
        );

        // check for first ping
        assert_eq!(sender.poll_timeout(), Some(time));
        assert_eq!(
            sender.pending_send(),
            Some(sunshine_ping(ping.clone(), 0).as_slice())
        );

        // consume ping
        sender.consume_send();
        assert_eq!(sender.pending_send(), None);
        assert_eq!(sender.poll_timeout(), Some(time + PING_RETRY_TIMEOUT));

        // advance time only by half
        time += PING_RETRY_TIMEOUT / 2;
        sender.handle_timeout(time);

        // check for no ping
        assert_eq!(sender.poll_timeout(), Some(time + (PING_RETRY_TIMEOUT / 2)));
        assert_eq!(sender.pending_send(), None);

        // advance time by another half
        time += PING_RETRY_TIMEOUT / 2;
        sender.handle_timeout(time);

        // check for second ping
        assert_eq!(sender.poll_timeout(), Some(time));
        assert_eq!(
            sender.pending_send(),
            Some(sunshine_ping(ping.clone(), 1).as_slice())
        );

        // consume ping
        sender.consume_send();
        assert_eq!(sender.pending_send(), None);
        assert_eq!(sender.poll_timeout(), Some(time + PING_RETRY_TIMEOUT));
    }
}
