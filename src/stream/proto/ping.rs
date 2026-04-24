use std::time::{Duration, Instant};

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

pub enum PingSenderInput {
    Timeout(Instant),
}
pub enum PingSenderOutput<'a> {
    Send {
        data: &'a [u8],
    },
    Timeout(Instant),
    /// When this is returned, the ping sender can be discarded
    Finished,
}

#[derive(Debug)]
pub struct PingSender {
    last_now: Instant,
    last_ping_send: Option<Instant>,
    config: PingSenderConfig,
    state: PingSenderState,
    current_ping_packet: [u8; SunshinePingPacket::SIZE],
}

impl PingSender {
    #[instrument(level = Level::DEBUG)]
    pub fn new(now: Instant, config: PingSenderConfig) -> Self {
        Self {
            last_now: now,
            last_ping_send: None,
            config,
            state: PingSenderState::Pinging { last_attempt: None },
            current_ping_packet: [0; _],
        }
    }

    pub fn handle_input(&mut self, input: PingSenderInput) {
        match input {
            PingSenderInput::Timeout(now) => {
                self.last_now = now;
            }
        }
    }

    pub fn poll_output(&mut self) -> PingSenderOutput<'_> {
        match &mut self.state {
            PingSenderState::Pinging { last_attempt } => {
                // Check if we've reached the timeout
                if let Some(last_ping_send) = self.last_ping_send {
                    let duration_since_last_ping = self.last_now.duration_since(last_ping_send);

                    if duration_since_last_ping < PING_RETRY_TIMEOUT {
                        return PingSenderOutput::Timeout(
                            self.last_now + (PING_RETRY_TIMEOUT - duration_since_last_ping),
                        );
                    }
                }

                // Send Ping
                let current_attempt = last_attempt.map(|x| x + 1).unwrap_or(0);

                let packet_len = if let Some(ping) = self.config.sunshine_ping.as_ref() {
                    // Use Sunshine ping
                    let packet = SunshinePingPacket {
                        payload: ping.clone(),
                        sequence_number: current_attempt as u32,
                    };

                    packet.serialize(&mut self.current_ping_packet);
                    SunshinePingPacket::SIZE
                } else {
                    // Just some magic bytes
                    let magic = [0x50, 0x49, 0x4E, 0x47];

                    self.current_ping_packet[0..magic.len()].copy_from_slice(&magic);
                    magic.len()
                };

                let packet = &self.current_ping_packet[0..packet_len];
                debug!(packet = ?packet, "sending ping");

                *last_attempt = Some(current_attempt);
                self.last_ping_send = Some(self.last_now);

                PingSenderOutput::Send {
                    data: &self.current_ping_packet,
                }
            }
            PingSenderState::Finished => PingSenderOutput::Finished,
        }
    }

    pub fn state(&self) -> PingSenderState {
        self.state
    }

    pub fn set_finished(&mut self) {
        self.state = PingSenderState::Finished;
    }
}
