use std::{
    error::Error,
    fmt::{self, Debug, Formatter},
    mem,
    net::SocketAddr,
    time::Instant,
};

use rusty_enet::{PacketKind, error::PeerSendError};
use tracing::{Level, debug, instrument, trace, warn};

use crate::{
    ServerVersion,
    stream::{
        AesKey,
        proto::{
            control::{
                packet::{
                    ControlPacket, ControlPacketConfig, PERIODIC_PING_INTERVAL,
                    PERIODIC_PING_VERSION,
                },
                peer::{
                    ControlConnectConfig, ControlEncryptionMethod, ControlError, ControlHost,
                    ControlHostAction, ControlHostConfig, ControlHostEvent, ControlHostInput,
                    ControlHostOutput, ControlPeerConfig, ControlPeerId, ControlPeerRole,
                },
            },
            crypto::CryptoBackend,
            enet::EnetError,
        },
    },
};

pub mod packet;

mod encryption;
pub mod peer;

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod test;

// TODO: where's the difference between v1 and v2 headers?

const CHANNEL_GENERIC: u8 = 0x00;
const CHANNEL_URGENT: u8 = 0x01; // IDR and reference frame invalidation requests
const CHANNEL_KEYBOARD: u8 = 0x02;
const CHANNEL_MOUSE: u8 = 0x03;
const CHANNEL_PEN: u8 = 0x04;
const CHANNEL_TOUCH: u8 = 0x05;
const CHANNEL_UTF8: u8 = 0x06;
const CHANNEL_GAMEPAD_BASE: u8 = 0x10; // 0x10 to 0x1F by controller index
const CHANNEL_SENSOR_BASE: u8 = 0x20; // 0x20 to 0x2F by controller index
const CHANNEL_COUNT: usize = 0x30;

/// A message from the [MoonlightStreamProto](super::MoonlightStreamProto) to the [ControlStream]
#[derive(Debug)]
pub struct ControlMessage(pub(super) ControlMessageInner);
#[derive(Debug)]
pub(super) enum ControlMessageInner {
    /// The first packets MUST be RequestIdr, followed by StartB on Sunshine
    /// Only allow other packets (e.g. ping, actions) after the starting process from the main stream is done
    AllowOtherPackets,
    /// Sends a packet regardless of the [Self::AllowOtherPackets] option
    SendPacket { packet: ControlPacket, force: bool },
}

#[derive(Debug)]
pub struct ControlStreamConfig {
    pub server_version: ServerVersion,
    pub addr: SocketAddr,
    pub sunshine_connect_data: Option<u32>,
    pub encryption: Option<(ControlEncryptionMethod, AesKey)>,
}

#[derive(Debug)]
pub enum ControlStreamInput<'a> {
    // TODO: don't use the host input, but put this into the enum or another new enum
    Host(ControlHostInput<'a>),
    /// A message received from the main [MoonlightStreamProto](super::MoonlightStreamProto) or the [VideoStream](super::video::VideoStream)
    Message {
        now: Instant,
        message: ControlMessage,
    },
}

#[derive(Debug)]
pub enum ControlStreamOutput {
    Action(ControlHostAction),
    Event(ControlStreamEvent),
}

#[derive(Debug)]
pub enum ControlStreamEvent {
    /// The control has successfully connected to the server:
    /// - Packets can now be sent
    Connect,
    /// The [ControlStream] received a packet.
    Packet(ControlPacket),
    /// The control stream got disconnected
    Disconnect,
}

pub struct ControlStream<Crypto> {
    server_version: ServerVersion,
    peer: ControlPeerId,
    peer_connected: bool,
    allow_packets: bool,
    last_now: Instant,
    last_ping: Option<Instant>,
    buffered_packets: Vec<ControlPacket>,
    host: ControlHost<Crypto>,
}

impl<Crypto> ControlStream<Crypto>
where
    Crypto: CryptoBackend,
    Crypto::Error: Error + 'static,
{
    #[instrument(level = Level::DEBUG, skip(crypto_backend))]
    pub fn new(
        now: Instant,
        config: ControlStreamConfig,
        crypto_backend: Crypto,
    ) -> Result<Self, ControlError> {
        if config.server_version.major < 5 {
            // Servers below v5 use tcp and don't have encryption support
            // https://github.com/moonlight-stream/moonlight-common-c/blob/7b026e77be62175104640e7e722b758df6d3d0d7/src/ControlStream.c#L849-L856
            return Err(ControlError::VersionNotSupported(config.server_version));
        }

        // All values that could lead to an error are controlled by us and won't cause errors
        // -> This cannot fail
        #[allow(clippy::unwrap_used)]
        let mut host = ControlHost::new(
            now,
            ControlHostConfig {
                peer_count: 1,
                peer_channel_count: CHANNEL_COUNT,
            },
            crypto_backend,
        )
        .unwrap();

        let packets = ControlPacketConfig::new(config.server_version, false)
            .ok_or(ControlError::VersionNotSupported(config.server_version))?;

        // All values that could lead to an error are controlled by us and won't cause errors
        // -> This cannot fail
        #[allow(clippy::unwrap_used)]
        let peer = host
            .connect(
                config.addr,
                ControlConnectConfig {
                    channel_count: CHANNEL_COUNT,
                    sunshine_connect_data: config.sunshine_connect_data,
                    config: ControlPeerConfig {
                        role: ControlPeerRole::Client,
                        encryption: config.encryption,
                        packets,
                    },
                },
            )
            .unwrap();

        Ok(Self {
            server_version: config.server_version,
            peer,
            peer_connected: false,
            allow_packets: false,
            last_now: now,
            last_ping: (config.server_version >= PERIODIC_PING_VERSION).then_some(now),
            buffered_packets: vec![],
            host,
        })
    }

    pub fn send(&mut self, packet: ControlPacket) -> Result<(), ControlError> {
        self.send_inner(packet, false)
    }
    fn send_inner(
        &mut self,
        packet: ControlPacket,
        force_packet: bool,
    ) -> Result<(), ControlError> {
        if !force_packet && !self.allow_packets {
            return Err(ControlError::NotConnected);
        } else if force_packet && !self.peer_connected {
            self.buffered_packets.push(packet);
            return Ok(());
        }

        let (channel, kind) = if self.server_version.is_sunshine_like() {
            match packet {
                // request idr: https://github.com/moonlight-stream/moonlight-common-c/blob/62687809b1f7410c3db4be2527503a54ae408d70/src/ControlStream.c#L1522-L1528
                // ltr ack: https://github.com/moonlight-stream/moonlight-common-c/blob/62687809b1f7410c3db4be2527503a54ae408d70/src/ControlStream.c#L1569-L1575
                // invalidate ref frames: https://github.com/moonlight-stream/moonlight-common-c/blob/62687809b1f7410c3db4be2527503a54ae408d70/src/ControlStream.c#L1509-L1515
                ControlPacket::RequestIdr
                | ControlPacket::StartB
                | ControlPacket::LongTermReferenceFrameAcknowledgement { .. } => {
                    (CHANNEL_URGENT, PacketKind::Reliable)
                }
                // loss stats: https://github.com/moonlight-stream/moonlight-common-c/blob/62687809b1f7410c3db4be2527503a54ae408d70/src/ControlStream.c#L1469-L1475
                // frame fec: https://github.com/moonlight-stream/moonlight-common-c/blob/62687809b1f7410c3db4be2527503a54ae408d70/src/ControlStream.c#L1407-L1413
                ControlPacket::LossStats { .. } | ControlPacket::FrameFec { .. } => {
                    (CHANNEL_GENERIC, PacketKind::Unreliable { sequenced: false })
                }
                // See: https://github.com/moonlight-stream/moonlight-common-c/blob/2a5a1f3e8a57cbbb316ed7dfff3a3965c2e77d25/src/ControlStream.c#L1424-L1429
                // Send the message (and don't expect a response)
                //
                // NB: We send this periodic message as reliable to ensure the RTT is recomputed
                // regularly. This only happens when an ACK is received to a reliable packet.
                // Since the other traffic on this channel is unsequenced, it doesn't really
                // cause any negative HOL blocking side-effects.
                ControlPacket::PeriodicPing => (CHANNEL_GENERIC, PacketKind::Reliable),
                // https://github.com/moonlight-stream/moonlight-common-c/blob/62687809b1f7410c3db4be2527503a54ae408d70/src/InputStream.c#L738-L742
                ControlPacket::MouseMoveRelative { .. } => (CHANNEL_MOUSE, PacketKind::Reliable),
                // https://github.com/moonlight-stream/moonlight-common-c/blob/62687809b1f7410c3db4be2527503a54ae408d70/src/InputStream.c#L803-L806
                ControlPacket::MouseMoveAbsolute { .. } => (CHANNEL_MOUSE, PacketKind::Reliable),
                // https://github.com/moonlight-stream/moonlight-common-c/blob/62687809b1f7410c3db4be2527503a54ae408d70/src/InputStream.c#L865-L866
                ControlPacket::MouseButton { .. } => (CHANNEL_MOUSE, PacketKind::Reliable),
                // https://github.com/moonlight-stream/moonlight-common-c/blob/62687809b1f7410c3db4be2527503a54ae408d70/src/InputStream.c#L899-L900
                ControlPacket::Keyboard { .. } => (CHANNEL_KEYBOARD, PacketKind::Reliable),
                // https://github.com/moonlight-stream/moonlight-common-c/blob/62687809b1f7410c3db4be2527503a54ae408d70/src/InputStream.c#L980-L981
                ControlPacket::Text { .. } => (CHANNEL_UTF8, PacketKind::Reliable),
                _ => todo!("{:?}", packet),
            }
        } else {
            // https://github.com/moonlight-stream/moonlight-common-c/blob/2a5a1f3e8a57cbbb316ed7dfff3a3965c2e77d25/src/ControlStream.c#L763-L767
            // Always use channel 0 and reliable for GFE
            (0, PacketKind::Reliable)
        };

        // TODO: what channel?
        self.host.send(self.peer, channel, kind, packet)?;

        Ok(())
    }

    pub fn poll_output(&mut self) -> Result<ControlStreamOutput, ControlError> {
        if self.peer_connected {
            debug_assert_eq!(self.buffered_packets.len(), 0);
        }
        if self.host.can_discard() {
            // This only happens when there's no peer in the connection
            // -> we must've disconnected somehow -> this object is not useable anymore
            return Err(ControlError::NotConnected);
        }

        let mut timeout = loop {
            let output = self.host.poll_output()?;

            match output {
                ControlHostOutput::Action(ControlHostAction::Timeout(timeout)) => {
                    break timeout;
                }
                ControlHostOutput::Action(action) => {
                    return Ok(ControlStreamOutput::Action(action));
                }
                ControlHostOutput::Event(ControlHostEvent::Connected {
                    id,
                    sunshine_connect_data: _,
                }) => {
                    if id != self.peer {
                        // Nobody should connect to this peer, but if they do just instantly disconnect them
                        let _ = self.host.disconnect_now(id, 0);
                        continue;
                    }

                    self.peer_connected = true;

                    // Send all buffered packets
                    for packet in mem::take(&mut self.buffered_packets) {
                        self.send(packet)?;
                    }
                    continue;
                }
                ControlHostOutput::Event(ControlHostEvent::Receive {
                    id,
                    // TODO: is channel_id important?
                    channel_id: _,
                    packet,
                }) => {
                    if id != self.peer {
                        // ignore other peers
                        continue;
                    }

                    #[allow(clippy::single_match)]
                    match &packet {
                        ControlPacket::ServerTermination { .. } => {
                            // Disconnect instanly:
                            // We used to wait for a ENET_EVENT_TYPE_DISCONNECT event, but since
                            // GFE 3.20.3.63 we don't get one for 10 seconds after we first get
                            // this termination message. The termination message should be reliable
                            // enough to end the stream now, rather than waiting for an explicit
                            // disconnect. The server will also not acknowledge our disconnect
                            // message once it sends this message, so we mark the peer as fully
                            // disconnected now to avoid delays waiting for an ack that will
                            // never arrive.
                            // https://github.com/moonlight-stream/moonlight-common-c/blob/62687809b1f7410c3db4be2527503a54ae408d70/src/ControlStream.c#L1362-L1375
                            self.host.disconnect_now(self.peer, 0)?;

                            // The enet disconnect event will be called next poll
                        }
                        _ => {}
                    }

                    return Ok(ControlStreamOutput::Event(ControlStreamEvent::Packet(
                        packet,
                    )));
                }
                ControlHostOutput::Event(ControlHostEvent::Disconnected { id }) => {
                    if id != self.peer {
                        // ignore other peers
                        continue;
                    }

                    self.peer_connected = false;

                    return Ok(ControlStreamOutput::Event(ControlStreamEvent::Disconnect));
                }
            }
        };

        // Handle periodic ping
        if let Some(new_timeout) = self.do_ping()? {
            timeout = timeout.min(new_timeout);
        }

        Ok(ControlStreamOutput::Action(ControlHostAction::Timeout(
            timeout,
        )))
    }

    pub fn handle_input(&mut self, input: ControlStreamInput) -> Result<(), ControlError> {
        match input {
            ControlStreamInput::Host(input) => {
                let (ControlHostInput::Timeout(now) | ControlHostInput::Receive { now, .. }) =
                    &input;
                self.last_now = *now;

                self.host.handle_input(input)?;
            }
            ControlStreamInput::Message { now, message } => {
                self.last_now = now;

                self.host.handle_input(ControlHostInput::Timeout(now))?;

                self.handle_control_message(message)?;
            }
        }

        Ok(())
    }

    fn handle_control_message(&mut self, message: ControlMessage) -> Result<(), ControlError> {
        match message.0 {
            ControlMessageInner::SendPacket { packet, force } => {
                trace!(now = ?self.last_now, packet = ?packet, "received control message with packet");

                if force {
                    self.send_inner(packet, true)?;
                } else {
                    if let Err(err) = self.send(packet) {
                        trace!(error = ?err, "failed to send packet from control message");
                    }
                }
            }
            ControlMessageInner::AllowOtherPackets => {
                debug!(now = ?self.last_now, message = ?message, "received control message");

                self.allow_packets = true;
            }
        }

        Ok(())
    }

    /// Returns the time when the next ping must be sent
    fn do_ping(&mut self) -> Result<Option<Instant>, ControlError> {
        // If this server doesn't support the periodic ping
        let Some(last_ping) = self.last_ping else {
            trace!("server doesn't support periodic ping, not sending periodic ping");
            return Ok(None);
        };

        if self.last_now >= last_ping + PERIODIC_PING_INTERVAL {
            match self.send(ControlPacket::PeriodicPing) {
                Ok(()) => {}
                Err(ControlError::Enet(EnetError::PeerSendError(PeerSendError::NotConnected)))
                | Err(ControlError::NotConnected) => {
                    debug!(
                        self = ?self,
                        "not sending periodic ping because the control stream (via enet) is not connected yet."
                    );
                    // We are not connected yet -> we cannot send a ping
                    return Ok(None);
                }
                Err(err) => return Err(err),
            }

            trace!(
                last_ping = ?last_ping,
                now = ?self.last_now,
                "sending periodic ping"
            );

            self.last_ping = Some(self.last_now);
        }

        Ok(Some(last_ping + PERIODIC_PING_INTERVAL))
    }
}

impl<Crypto> Debug for ControlStream<Crypto> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "[ControlStream]")
    }
}
