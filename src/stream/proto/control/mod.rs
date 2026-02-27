use std::{
    fmt::{self, Debug, Formatter},
    mem::swap,
    net::SocketAddr,
    sync::Arc,
    time::Instant,
};

use rusty_enet::{Packet, PacketKind, PeerID, error::PeerSendError};
use thiserror::Error;
use tracing::{debug, trace, warn};

use crate::{
    ServerVersion,
    stream::{
        AesIv, AesKey,
        control::DEFAULT_CONTROL_PORT,
        proto::{
            control::packet::{
                ControlPacket, ControlPacketNotSupported, PERIODIC_PING_INTERVAL,
                PERIODIC_PING_VERSION,
            },
            crypto::CryptoContext,
            enet::{EnetConfig, EnetError, EnetEvent, EnetHost, EnetInput, EnetOutput},
        },
    },
};

pub(super) mod packet;

#[cfg(test)]
mod test;

// TODO: send loss stats: https://github.com/moonlight-stream/moonlight-common-c/blob/435bc6a5a4852c90cfb037de1378c0334ed36d8e/src/ControlStream.c#L1364-L1464

const CHANNEL_GENERIC: usize = 0x00;
const CHANNEL_URGENT: usize = 0x01; // IDR and reference frame invalidation requests
const CHANNEL_KEYBOARD: usize = 0x02;
const CHANNEL_MOUSE: usize = 0x03;
const CHANNEL_PEN: usize = 0x04;
const CHANNEL_TOUCH: usize = 0x05;
const CHANNEL_UTF8: usize = 0x06;
const CHANNEL_GAMEPAD_BASE: usize = 0x10; // 0x10 to 0x1F by controller index
const CHANNEL_SENSOR_BASE: usize = 0x20; // 0x20 to 0x2F by controller index
const CHANNEL_COUNT: usize = 0x30;

#[derive(Debug)]
pub struct ControlMessage {
    pub(super) packet: ControlPacket,
}

#[derive(Debug, Error)]
pub enum ControlStreamError {
    #[error("enet: {0}")]
    Enet(#[from] EnetError),
    #[error("packet not supported")]
    PacketNotSupported(ControlPacketNotSupported),
}

pub struct ControlStreamConfig {
    pub server_version: ServerVersion,
    pub addr: SocketAddr,
    pub sunshine_connect_data: Option<u32>,
    // TODO
    pub sunshine_encryption: Option<(AesKey, AesIv)>,
}

#[derive(Debug)]
pub enum ControlStreamInput<'a> {
    Timeout(Instant),
    /// A message received from the main MoonlightStream or the VideoStream
    Message(ControlMessage),
    Receive {
        now: Instant,
        addr: SocketAddr,
        data: &'a [u8],
    },
}

#[derive(Debug)]
pub enum ControlStreamOutput {
    Send { to: SocketAddr, data: Vec<u8> },
    Timeout(Instant),
    Event(ControlStreamEvent),
}

#[derive(Debug)]
pub enum ControlStreamEvent {
    OnPacket(ControlPacket),
}

enum Transport {
    Enet {
        enet: EnetHost,
        peer: Option<PeerID>,
        connected: bool,
        encrypted: Option<(Arc<dyn CryptoContext>, AesKey, AesIv)>,
    },
    Tcp {},
}

pub struct ControlStream {
    server_version: ServerVersion,
    addr: SocketAddr,
    last_now: Instant,
    transport: Transport,
    last_ping: Option<Instant>,
    // Buffered before the enet peer connected
    buffered_packets: Vec<(u8, Vec<u8>)>,
}

impl ControlStream {
    pub fn new(now: Instant, mut config: ControlStreamConfig) -> Self {
        if config.server_version < ServerVersion::new(5, 0, 0, 0) {
            // TODO: implement control over tcp

            config.sunshine_encryption = None;
            warn!(target: "moonlight_proto_control", "Tried to enable encryption on server version {:?} which doesn't have encryption support. Not using encryption!", config.server_version);
        }

        // https://github.com/moonlight-stream/moonlight-common-c/blob/3a377e7d7be7776d68a57828ae22283144285f90/src/ControlStream.c#L1713-L1737
        let mut enet = EnetHost::new(
            now,
            EnetConfig {
                channel_limit: CHANNEL_COUNT,
                peer_count: 1,
                incoming_bandwidth: None,
                outgoing_bandwidth: None,
            },
        );

        // All values that could lead to an error are controlled by us and won't cause errors
        // -> This cannot fail
        #[allow(clippy::unwrap_used)]
        let peer = enet
            .connect(
                config.addr,
                CHANNEL_COUNT,
                // https://github.com/moonlight-stream/moonlight-common-c/blob/3a377e7d7be7776d68a57828ae22283144285f90/src/RtspConnection.c#L1286-L1293
                config.sunshine_connect_data.unwrap_or(0),
            )
            .unwrap();

        Self {
            server_version: config.server_version,
            addr: config.addr,
            last_now: now,
            transport: Transport::Enet {
                enet,
                peer: Some(peer),
                connected: false,
                // TODO: encryption
                encrypted: config.sunshine_encryption.map(|x| todo!()),
            },
            last_ping: (config.server_version >= PERIODIC_PING_VERSION).then_some(now),
            buffered_packets: Vec::new(),
        }
    }

    pub fn send(&mut self, packet: ControlPacket) -> Result<(), ControlStreamError> {
        trace!(target: "moonlight_proto_control", "Sending Packet {packet:?}");

        let mut buffer = [0; _];

        let encrypted = matches!(
            self.transport,
            Transport::Enet {
                encrypted: Some(_),
                ..
            }
        );

        let len = packet.serialize(self.server_version, encrypted, &mut buffer);

        // TODO: what channel?
        self.send_raw(CHANNEL_GENERIC as u8, &buffer[0..len])?;

        Ok(())
    }
    fn send_raw(&mut self, channel_id: u8, buffer: &[u8]) -> Result<(), ControlStreamError> {
        // https://github.com/moonlight-stream/moonlight-common-c/blob/435bc6a5a4852c90cfb037de1378c0334ed36d8e/src/ControlStream.c#L822-L835

        match &mut self.transport {
            Transport::Enet {
                enet,
                peer,
                connected,
                encrypted,
            } => {
                if !*connected {
                    self.buffered_packets.push((channel_id, buffer.to_vec()));
                    return Ok(());
                }

                let Some(peer) = peer else {
                    // TODO: maybe error and disconnect?
                    return Ok(());
                };

                // TODO: encryption?

                let packet_kind = if self.server_version.is_sunshine_like() {
                    PacketKind::Unreliable { sequenced: false }
                } else {
                    PacketKind::Reliable
                };

                let peer = enet.peer(*peer).unwrap();

                if let Some(encryption) = &encrypted {
                    todo!()
                } else {
                    peer.send(channel_id, &Packet::new(buffer.to_vec(), packet_kind))
                        .map_err(EnetError::from)?;
                }
            }
            Transport::Tcp {} => {
                todo!();
            }
        }

        Ok(())
    }

    pub fn poll_output(&mut self) -> Result<ControlStreamOutput, ControlStreamError> {
        let output = self.do_poll()?;

        trace!(target: "moonlight_proto_control", "Output: {output:?}");

        Ok(output)
    }
    fn do_poll(&mut self) -> Result<ControlStreamOutput, ControlStreamError> {
        let mut timeout = loop {
            match &mut self.transport {
                Transport::Enet {
                    enet,
                    peer,
                    connected,
                    encrypted,
                } => match enet.poll_output()? {
                    EnetOutput::Send { addr, data } => {
                        return Ok(ControlStreamOutput::Send { to: addr, data });
                    }
                    EnetOutput::Event(EnetEvent::Connect {
                        peer: event_peer,
                        data,
                    }) => {
                        if *peer == Some(event_peer) {
                            *connected = true;

                            // Send buffered packets
                            let mut packets = Vec::new();
                            swap(&mut self.buffered_packets, &mut packets);
                            for (channel_id, buffer) in packets {
                                self.send_raw(channel_id, &buffer)?;
                            }
                            debug_assert_eq!(self.buffered_packets.len(), 0);
                        }
                        continue;
                    }
                    EnetOutput::Event(EnetEvent::Receive {
                        peer,
                        channel_id,
                        data,
                    }) => {
                        if let Some(encryption) = encrypted {
                            // TODO: implement encryption
                            todo!();
                        }

                        let Some(packet) = ControlPacket::deserialize(
                            self.server_version,
                            encrypted.is_some(),
                            &data,
                        ) else {
                            warn!(target:"moonlight_proto_control", "Failed to deserialize control packet!");

                            trace!(target: "moonlight_proto_control", "Failed to deserialize control packet: Peer: {peer:?}, Channel: {channel_id}, Data: {data:?}");
                            continue;
                        };

                        return Ok(ControlStreamOutput::Event(ControlStreamEvent::OnPacket(
                            packet,
                        )));
                    }
                    EnetOutput::Event(EnetEvent::Disconnect {
                        peer: event_peer,
                        data,
                    }) => {
                        if peer.is_some_and(|peer| peer == event_peer) {
                            *peer = None;
                        }

                        // TODO: what does the data mean?
                        todo!();
                        continue;
                    }
                    EnetOutput::Timeout(timeout) => break timeout,
                },
                Transport::Tcp {} => {
                    todo!();
                }
            }
        };

        // Handle periodic ping
        if let Some(new_timeout) = self.do_ping()? {
            timeout = timeout.min(new_timeout);
        }

        Ok(ControlStreamOutput::Timeout(timeout))
    }

    pub fn handle_input(&mut self, input: ControlStreamInput) -> Result<(), ControlStreamError> {
        trace!(target: "moonlight_proto_control", "Input: {input:?}");

        match &mut self.transport {
            Transport::Enet { enet, .. } => match input {
                ControlStreamInput::Timeout(timeout) => {
                    self.last_now = timeout;

                    enet.handle_input(EnetInput::Timeout(timeout))?;
                }
                ControlStreamInput::Receive { now, addr, data } => {
                    self.last_now = now;

                    if addr != self.addr {
                        enet.handle_input(EnetInput::Timeout(now))?;

                        return Ok(());
                    }

                    enet.handle_input(EnetInput::Receive { now, addr, data })?;
                }
                ControlStreamInput::Message(message) => {
                    self.send(message.packet).unwrap();
                }
            },
            Transport::Tcp {} => {
                todo!();
            }
        }

        Ok(())
    }

    /// Returns the time when the next ping must be sent
    fn do_ping(&mut self) -> Result<Option<Instant>, ControlStreamError> {
        // If this server doesn't support the periodic ping
        let Some(last_ping) = self.last_ping else {
            return Ok(None);
        };

        if self.last_now >= last_ping + PERIODIC_PING_INTERVAL {
            match self.send(ControlPacket::PeriodicPing) {
                Ok(()) => {}
                Err(ControlStreamError::Enet(EnetError::PeerSendError(
                    PeerSendError::NotConnected,
                ))) => {
                    debug!(target: "moonlight_proto_control", "Not sending periodic ping because the control stream (via enet) is not connected yet.");
                    // We are not connected yet -> we cannot send a ping
                    return Ok(None);
                }
                Err(err) => return Err(err),
            }
            trace!(target: "moonlight_proto_control", "Sending Periodic Ping");

            self.last_ping = Some(self.last_now);
        }

        Ok(Some(last_ping + PERIODIC_PING_INTERVAL))
    }
}

impl Debug for ControlStream {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "[ControlStream]")
    }
}
