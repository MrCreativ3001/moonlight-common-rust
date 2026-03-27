use std::{
    error::Error,
    fmt::{self, Debug, Formatter},
    mem::swap,
    net::SocketAddr,
    time::Instant,
};

use rusty_enet::{Packet, PacketKind, PeerID, error::PeerSendError};
use thiserror::Error;
use tracing::{Level, debug, instrument, trace, trace_span, warn};

use crate::{
    ServerVersion,
    stream::{
        AesIv, AesKey,
        proto::{
            control::{
                encryption::{
                    ControlEncryptionError, decrypt_server_control_packet_into,
                    encrypt_client_control_packet_into,
                },
                packet::{
                    ControlPacket, ControlPacketNotSupported,
                    ENCRYPTED_CONTROL_PACKET_AES_GCM_TAG_LENGTH, ENCRYPTED_CONTROL_PACKET_TYPE,
                    EncryptedControlHeader, PERIODIC_PING_INTERVAL, PERIODIC_PING_VERSION,
                    PacketDirection,
                },
            },
            crypto::{CipherAlgorithm, CryptoBackend},
            enet::{EnetConfig, EnetError, EnetEvent, EnetHost, EnetInput, EnetOutput},
        },
    },
};

// TODO: make this possible to use on the server and client

pub mod packet;

mod encryption;

#[cfg(test)]
mod test;

// TODO: send loss stats: https://github.com/moonlight-stream/moonlight-common-c/blob/435bc6a5a4852c90cfb037de1378c0334ed36d8e/src/ControlStream.c#L1364-L1464

// TODO: where's the difference between v1 and v2 headers?

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

/// A message from the [MoonlightStreamProto](super::MoonlightStreamProto) to the [ControlStream]
#[derive(Debug)]
pub struct ControlMessage(pub(super) ControlMessageInner);
#[derive(Debug)]
pub(super) enum ControlMessageInner {
    /// The first packets MUST be RequestIdr, followed by StartB on Sunshine
    /// Only allow other packets (e.g. ping, actions) after the starting process from the main stream is done
    AllowOtherPackets,
    /// Sends a packet regardless of the [Self::AllowOtherPackets] option
    SendPacket { packet: ControlPacket },
}

#[derive(Debug, Error)]
pub enum ControlStreamError {
    #[error("enet: {0}")]
    Enet(#[from] EnetError),
    #[error("the control stream hasn't successfully connected yet")]
    NotConnected,
    #[error("packet not supported")]
    PacketNotSupported(#[from] ControlPacketNotSupported),
    #[error("encryption: {0}")]
    Encryption(#[from] ControlEncryptionError),
}

#[derive(Debug, Clone, Copy)]
pub enum ControlEncryptionMethod {
    /// Used for nvidia control encryption.
    /// Prefer [Sunshine](Self::Sunshine) over this because it's more secure.
    ///
    /// Enabled if APP_VERSION_AT_LEAST(7, 1, 431)
    ///
    /// References:
    /// - Server Version: https://github.com/moonlight-stream/moonlight-common-c/blob/62687809b1f7410c3db4be2527503a54ae408d70/src/ControlStream.c#L309
    /// - Encryption: https://github.com/moonlight-stream/moonlight-common-c/blob/62687809b1f7410c3db4be2527503a54ae408d70/src/ControlStream.c#L568-L574
    Nvidia,
    /// Enabled if [SunshineEncryptionFlags::CONTROL_V2](super::sdp::client::SunshineEncryptionFlags::CONTROL_V2).
    Sunshine,
}

#[derive(Debug)]
pub struct ControlStreamConfig {
    pub server_version: ServerVersion,
    pub addr: SocketAddr,
    pub sunshine_connect_data: Option<u32>,
    pub encryption: Option<(ControlEncryptionMethod, AesKey, AesIv)>,
}

#[derive(Debug)]
pub enum ControlStreamInput<'a> {
    Timeout(Instant),
    /// A message received from the main [MoonlightStreamProto](super::MoonlightStreamProto) or the [VideoStream](super::video::VideoStream)
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
    /// The control has successfully connected to the server:
    /// - Packets can now be sent
    Connect,
    /// The [ControlStream] received a packet.
    Packet(ControlPacket),
}

struct EnetEncrypted {
    encryption_method: ControlEncryptionMethod,
    aes_key: AesKey,
    aes_iv: AesIv,
    send_sequence_number: u32,
    encrypt_buffer: Vec<u8>,
}

enum Transport {
    Enet {
        enet: EnetHost,
        peer: Option<PeerID>,
        connected: bool,
        encryption: Option<EnetEncrypted>,
    },
    Tcp {},
}

pub struct ControlStream<Crypto> {
    server_version: ServerVersion,
    addr: SocketAddr,
    crypto_backend: Crypto,
    last_now: Instant,
    transport: Transport,
    allow_packets: bool,
    last_ping: Option<Instant>,
    // Buffered before the enet peer connected
    buffered_packets: Vec<(u8, Vec<u8>)>,
}

impl<Crypto> ControlStream<Crypto>
where
    Crypto: CryptoBackend,
    Crypto::Error: Error + 'static,
{
    #[instrument(level = Level::DEBUG, skip(crypto_backend))]
    pub fn new(now: Instant, mut config: ControlStreamConfig, crypto_backend: Crypto) -> Self {
        if config.server_version < ServerVersion::new(5, 0, 0, 0) {
            // TODO: implement control over tcp

            config.encryption = None;
            warn!(
                "Tried to enable encryption on server version {:?} which doesn't have encryption support. Not using encryption!",
                config.server_version
            );
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
            crypto_backend,
            last_now: now,
            transport: Transport::Enet {
                enet,
                peer: Some(peer),
                connected: false,
                // TODO: encryption
                encryption: config
                    .encryption
                    .map(|(encryption_method, aes_key, aes_iv)| EnetEncrypted {
                        encryption_method,
                        aes_key,
                        aes_iv,
                        send_sequence_number: 0,
                        encrypt_buffer: vec![
                            0;
                            EncryptedControlHeader::SIZE + ControlPacket::MAX_SIZE
                        ],
                    }),
            },
            allow_packets: false,
            last_ping: (config.server_version >= PERIODIC_PING_VERSION).then_some(now),
            buffered_packets: Vec::new(),
        }
    }

    pub fn send(&mut self, packet: ControlPacket) -> Result<(), ControlStreamError> {
        self.send_inner(packet, false)
    }
    fn send_inner(
        &mut self,
        packet: ControlPacket,
        force_packet: bool,
    ) -> Result<(), ControlStreamError> {
        // Avoid spam from ping
        if !matches!(packet, ControlPacket::PeriodicPing) {
            debug!(packet = ?packet, "Sending Packet");
        }

        if !force_packet && !self.allow_packets {
            return Err(ControlStreamError::NotConnected);
        }

        let mut buffer = [0; _];

        let packet_kind = if self.server_version.is_sunshine_like() {
            match packet {
                // TODO: are those reliable?
                ControlPacket::RequestIdr => PacketKind::Reliable,
                ControlPacket::StartB => PacketKind::Reliable,
                // See: https://github.com/moonlight-stream/moonlight-common-c/blob/2a5a1f3e8a57cbbb316ed7dfff3a3965c2e77d25/src/ControlStream.c#L1424-L1429
                // Send the message (and don't expect a response)
                //
                // NB: We send this periodic message as reliable to ensure the RTT is recomputed
                // regularly. This only happens when an ACK is received to a reliable packet.
                // Since the other traffic on this channel is unsequenced, it doesn't really
                // cause any negative HOL blocking side-effects.
                ControlPacket::PeriodicPing => PacketKind::Reliable,
                _ => PacketKind::Unreliable { sequenced: false },
            }
        } else {
            PacketKind::Reliable
        };

        let channel = if self.server_version.is_sunshine_like() {
            (match packet {
                ControlPacket::RequestIdr | ControlPacket::StartB => CHANNEL_URGENT,
                _ => CHANNEL_GENERIC,
            }) as u8
        } else {
            // https://github.com/moonlight-stream/moonlight-common-c/blob/2a5a1f3e8a57cbbb316ed7dfff3a3965c2e77d25/src/ControlStream.c#L763-L767
            // Always use channel 0 for GFE
            0
        };

        let encrypted = matches!(
            self.transport,
            Transport::Enet {
                encryption: Some(_),
                ..
            }
        );

        let len = packet.serialize(self.server_version, encrypted, &mut buffer)?;

        // TODO: what channel?
        self.send_raw(packet_kind, channel, &buffer[0..len], force_packet)?;

        Ok(())
    }
    #[instrument(level = Level::TRACE)]
    fn send_raw(
        &mut self,
        packet_kind: PacketKind,
        mut channel_id: u8,
        buffer: &[u8],
        force_packet: bool,
    ) -> Result<(), ControlStreamError> {
        // https://github.com/moonlight-stream/moonlight-common-c/blob/435bc6a5a4852c90cfb037de1378c0334ed36d8e/src/ControlStream.c#L822-L835

        match &mut self.transport {
            Transport::Enet {
                enet,
                peer,
                connected,
                encryption,
            } => {
                if !*connected {
                    if !force_packet {
                        return Err(ControlStreamError::NotConnected);
                    }

                    trace!(channel_id = channel_id, packet_data = ?buffer, "Buffering Packet");

                    self.buffered_packets.push((channel_id, buffer.to_vec()));
                    return Ok(());
                }

                // Handle encryption
                // https://github.com/moonlight-stream/moonlight-common-c/blob/62687809b1f7410c3db4be2527503a54ae408d70/src/ControlStream.c#L703-L740
                // https://github.com/moonlight-stream/moonlight-common-c/blob/62687809b1f7410c3db4be2527503a54ae408d70/src/ControlStream.c#L548-L591
                let buffer = if let Some(EnetEncrypted {
                    encryption_method,
                    aes_key,
                    aes_iv: _,
                    send_sequence_number,
                    encrypt_buffer,
                }) = encryption
                {
                    let len = encrypt_client_control_packet_into(
                        &self.crypto_backend,
                        *encryption_method,
                        *aes_key,
                        *send_sequence_number,
                        buffer,
                        encrypt_buffer,
                    )?;

                    &encrypt_buffer[0..len]
                } else {
                    buffer
                };

                let Some(peer) = peer else {
                    // TODO: maybe error and disconnect?
                    return Ok(());
                };

                // TODO: encryption?

                let peer = enet.peer(*peer).unwrap();

                // https://github.com/moonlight-stream/moonlight-common-c/blob/2a5a1f3e8a57cbbb316ed7dfff3a3965c2e77d25/src/ControlStream.c#L763-L767
                // if the requested channel exceeds the peer's supported channel count.
                if channel_id as usize >= peer.channel_count() {
                    channel_id = 0;
                }

                peer.send(channel_id, &Packet::new(buffer.to_vec(), packet_kind))
                    .map_err(EnetError::from)?;
            }
            Transport::Tcp {} => {
                todo!();
            }
        }

        Ok(())
    }

    pub fn poll_output(&mut self) -> Result<ControlStreamOutput, ControlStreamError> {
        let mut timeout = loop {
            match &mut self.transport {
                Transport::Enet {
                    enet,
                    peer,
                    connected,
                    encryption,
                } => match enet.poll_output()? {
                    EnetOutput::Send { addr, data } => {
                        return Ok(ControlStreamOutput::Send { to: addr, data });
                    }
                    EnetOutput::Event(EnetEvent::Connect {
                        peer: event_peer,
                        data: _,
                    }) => {
                        if *peer == Some(event_peer) {
                            *connected = true;

                            // Send buffered packets
                            let span = trace_span!("send_buffered_packets");
                            let enter = span.enter();

                            let mut packets = Vec::new();
                            swap(&mut self.buffered_packets, &mut packets);

                            for (channel_id, buffer) in packets.drain(..) {
                                trace!(channel_id = channel_id, packet_data = ?buffer, "Sending buffered packet");
                                self.send_raw(PacketKind::Reliable, channel_id, &buffer, true)?;
                            }

                            debug_assert_eq!(self.buffered_packets.len(), 0);
                            debug_assert_eq!(packets.len(), 0);

                            drop(enter);
                        }
                        continue;
                    }
                    EnetOutput::Event(EnetEvent::Receive {
                        peer,
                        channel_id,
                        mut data,
                    }) => {
                        trace!(peer_id = ?peer, channel_id = ?channel_id, data = ?data, "Received raw packet");

                        let is_encrypted = encryption.is_some();
                        // Encryption:
                        // https://github.com/moonlight-stream/moonlight-common-c/blob/62687809b1f7410c3db4be2527503a54ae408d70/src/ControlStream.c#L1219-L1253
                        // https://github.com/moonlight-stream/moonlight-common-c/blob/62687809b1f7410c3db4be2527503a54ae408d70/src/ControlStream.c#L593-L663
                        let data = if let Some(EnetEncrypted {
                            encryption_method,
                            aes_key,
                            aes_iv: _,
                            send_sequence_number: _,
                            encrypt_buffer,
                        }) = encryption
                        {
                            // The encrypt buffer is always bigger than the required decrypt buffer
                            // -> The buffer is big enough for decrypting

                            // TODO: some errors should drop packets and not error the consumer of this stream
                            let len = decrypt_server_control_packet_into(
                                &self.crypto_backend,
                                *encryption_method,
                                *aes_key,
                                &data,
                                encrypt_buffer,
                            )?;

                            &encrypt_buffer[0..len]
                        } else {
                            &data
                        };

                        let Some(packet) = ControlPacket::deserialize(
                            PacketDirection::ClientBound,
                            self.server_version,
                            is_encrypted,
                            data,
                        ) else {
                            warn!("Failed to deserialize control packet!");

                            trace!(
                                "Failed to deserialize control packet: Peer: {peer:?}, Channel: {channel_id}, Data: {data:?}"
                            );
                            continue;
                        };

                        debug!(packet = ?packet, "Received Packet");

                        return Ok(ControlStreamOutput::Event(ControlStreamEvent::Packet(
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
                ControlStreamInput::Message(ControlMessage(inner)) => {
                    debug!(control_message = ?inner, "Received message from main stream");

                    match inner {
                        ControlMessageInner::SendPacket { packet } => {
                            self.send_inner(packet, true)?;
                        }
                        ControlMessageInner::AllowOtherPackets => {
                            self.allow_packets = true;
                        }
                    }
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
                )))
                | Err(ControlStreamError::NotConnected) => {
                    debug!(
                        "Not sending periodic ping because the control stream (via enet) is not connected yet."
                    );
                    // We are not connected yet -> we cannot send a ping
                    return Ok(None);
                }
                Err(err) => return Err(err),
            }

            trace!(
                last_ping = ?last_ping,
                now = ?self.last_now,
                "Sending Periodic Ping"
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
