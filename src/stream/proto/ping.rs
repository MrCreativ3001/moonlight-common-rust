use std::time::Duration;

use sans_io_time::Instant;

use smallvec::{SmallVec, smallvec};
use tracing::{Level, debug, instrument};

use crate::stream::proto::packet::{SunshinePing, SunshinePingPacket};

pub const PING_RETRY_TIMEOUT: Duration = Duration::from_millis(500);

#[derive(Debug)]
pub struct PingSenderConfig {
    pub sunshine_ping: Option<SunshinePing>,
}

#[derive(Debug, Clone, Copy)]
pub enum PingSenderState {
    Pinging { last_attempt: Option<usize> },
    Finished,
}

#[derive(Debug)]
pub struct PingSender {
    initial_time: Instant,
    last_ping_send: Option<Instant>,
    config: PingSenderConfig,
    state: PingSenderState,
    current_ping_packet: SmallVec<[u8; SunshinePingPacket::SIZE]>,
}

impl PingSender {
    #[instrument(level = Level::DEBUG)]
    pub fn new(now: Instant, config: PingSenderConfig) -> Self {
        Self {
            initial_time: now,
            last_ping_send: None,
            config,
            state: PingSenderState::Pinging { last_attempt: None },
            current_ping_packet: smallvec![],
        }
    }

    pub fn poll_timeout(&self) -> Option<Instant> {
        if matches!(self.state, PingSenderState::Finished) {
            return None;
        }

        Some(
            self.last_ping_send
                .map(|last_ping_send| last_ping_send + PING_RETRY_TIMEOUT)
                .unwrap_or(self.initial_time),
        )
    }

    pub fn pending_send(&self) -> Option<&[u8]> {
        if self.current_ping_packet.is_empty() {
            None
        } else {
            Some(&self.current_ping_packet)
        }
    }
    pub fn consume_send(&mut self) {
        self.current_ping_packet.clear();
    }

    pub fn handle_timeout(&mut self, now: Instant) {
        match &mut self.state {
            PingSenderState::Pinging { last_attempt } => {
                // Check if we've reached the timeout
                if let Some(last_ping_send) = self.last_ping_send {
                    let duration_since_last_ping = now.duration_since(last_ping_send);

                    // Not reached the timeout yet
                    if duration_since_last_ping < PING_RETRY_TIMEOUT {
                        return;
                    }
                }

                // Send Ping
                let current_attempt = last_attempt.map(|x| x + 1).unwrap_or(0);

                self.current_ping_packet.resize(SunshinePingPacket::SIZE, 0);
                let current_ping_packet = self
                    .current_ping_packet
                    .as_mut_array()
                    .expect("array with ping packet size");

                let packet_len = if let Some(ping) = self.config.sunshine_ping.as_ref() {
                    // Use Sunshine ping
                    let packet = SunshinePingPacket {
                        payload: ping.clone(),
                        sequence_number: current_attempt as u32,
                    };

                    packet.serialize(current_ping_packet);
                    SunshinePingPacket::SIZE
                } else {
                    // Just some magic bytes
                    let magic = [0x50, 0x49, 0x4E, 0x47];

                    current_ping_packet[0..magic.len()].copy_from_slice(&magic);
                    magic.len()
                };
                self.current_ping_packet.truncate(packet_len);

                let packet = &self.current_ping_packet[0..packet_len];
                debug!(packet = ?packet, "sending ping");

                *last_attempt = Some(current_attempt);
                self.last_ping_send = Some(now);
            }
            PingSenderState::Finished => {
                // do nothing
            }
        }
    }

    pub fn state(&self) -> PingSenderState {
        self.state
    }

    pub fn set_finished(&mut self) {
        self.state = PingSenderState::Finished;
    }
}
