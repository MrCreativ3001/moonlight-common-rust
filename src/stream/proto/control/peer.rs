use std::{
    collections::{HashMap, VecDeque},
    net::SocketAddr,
};

use rusty_enet::{Packet, PacketKind, PeerID, PeerState};
use sans_io_time::Instant;
use thiserror::Error;
use tracing::{Level, debug, instrument, trace, warn};

use crate::{
    ServerVersion,
    stream::{
        AesKey,
        control::EstimatedRttInfo,
        proto::{
            DynCryptoBackend,
            control::{
                encryption::{
                    ControlEncryptionError, decrypt_clientbound_control_packet_into,
                    decrypt_serverbound_control_packet_into,
                    encrypt_clientbound_control_packet_into,
                    encrypt_serverbound_control_packet_into,
                },
                packet::{
                    ControlPacket, ControlPacketConfig, ControlPacketNotSupported,
                    EncryptedControlHeader, EnetChannel, PacketDirection,
                },
            },
            enet::{EnetConfig, EnetError, EnetEvent, EnetHost},
            runtime::UdpStream,
        },
    },
};

#[derive(Debug, Error)]
pub enum ControlError {
    #[error("this version of the protocol is not supported: {0}")]
    VersionNotSupported(ServerVersion),
    #[error("enet: {0}")]
    Enet(#[from] EnetError),
    #[error("the control stream hasn't successfully connected yet")]
    NotConnected,
    #[error("the peer was not configured, but this is required to do this action")]
    NotConfigured,
    #[error("packet not supported")]
    PacketNotSupported(#[from] ControlPacketNotSupported),
    /// Apollo Extension
    ///
    /// See also:
    /// - [ApolloPermissions](crate::stream::ApolloPermissions)
    #[error("the apollo permissions list doesn't allow this action")]
    ApolloPermissionDenied,
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
    /// - Server Version: <https://github.com/moonlight-stream/moonlight-common-c/blob/62687809b1f7410c3db4be2527503a54ae408d70/src/ControlStream.c#L309>
    /// - Encryption: <https://github.com/moonlight-stream/moonlight-common-c/blob/62687809b1f7410c3db4be2527503a54ae408d70/src/ControlStream.c#L568-L574>
    Nvidia,
    /// Enabled if [SunshineEncryptionFlags::CONTROL_V2](super::sdp::client::SunshineEncryptionFlags::CONTROL_V2).
    Sunshine,
}

#[derive(Debug)]
pub struct ControlHostConfig {
    /// The amount of peers this host can have
    pub peer_count: usize,
    /// The channel count per peer
    pub peer_channel_count: usize,
}

#[derive(Debug)]
pub struct ControlConnectConfig {
    pub channel_count: usize,
    /// Sunshine Extension
    pub sunshine_connect_data: Option<u32>,
    pub config: ControlPeerConfig,
}

#[derive(Debug)]
pub enum ControlPeerRole {
    Client,
    Server,
}

impl ControlPeerRole {
    pub fn incoming_direction(&self) -> PacketDirection {
        match self {
            Self::Client => PacketDirection::ClientBound,
            Self::Server => PacketDirection::ServerBound,
        }
    }
    pub fn outgoing_direction(&self) -> PacketDirection {
        match self {
            Self::Client => PacketDirection::ServerBound,
            Self::Server => PacketDirection::ClientBound,
        }
    }
}

#[derive(Debug)]
pub struct ControlPeerConfig {
    pub role: ControlPeerRole,
    pub encryption: Option<(ControlEncryptionMethod, AesKey)>,
    pub packets: ControlPacketConfig,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ControlPeerId(PeerID);

#[derive(Debug)]
pub enum ControlHostEvent {
    Connected {
        id: ControlPeerId,
        /// Sunshine Extension
        sunshine_connect_data: Option<u32>,
    },
    Receive {
        id: ControlPeerId,
        channel_id: u8,
        packet: ControlPacket,
    },
    Disconnected {
        id: ControlPeerId,
    },
}

#[derive(Debug)]
struct PeerData {
    send_sequence_number: u32,
    config: ControlPeerConfig,
}

pub struct ControlHost {
    crypto_backend: DynCryptoBackend,
    peer_data: HashMap<ControlPeerId, PeerData>,
    host: EnetHost,
    events: VecDeque<ControlHostEvent>,
}

impl ControlHost {
    #[instrument(level = Level::DEBUG, skip(crypto_backend))]
    pub fn new(
        now: Instant,
        config: ControlHostConfig,
        crypto_backend: DynCryptoBackend,
    ) -> Result<Self, ControlError> {
        Ok(Self {
            crypto_backend,
            peer_data: Default::default(),
            host: EnetHost::new(
                now,
                EnetConfig {
                    peer_count: config.peer_count,
                    channel_limit: config.peer_channel_count,
                    incoming_bandwidth: None,
                    outgoing_bandwidth: None,
                },
            ),
            events: Default::default(),
        })
    }

    #[instrument(level = Level::DEBUG, skip(self))]
    pub fn connect(
        &mut self,
        addr: SocketAddr,
        config: ControlConnectConfig,
    ) -> Result<ControlPeerId, ControlError> {
        let id = self.host.connect(
            addr,
            config.channel_count,
            config.sunshine_connect_data.unwrap_or(0),
        )?;

        let id = ControlPeerId(id);

        // This cannot fail because we just created that peer
        #[allow(clippy::unwrap_used)]
        self.configure_peer(id, config.config).unwrap();

        self.host.service();

        Ok(id)
    }

    /// This should directly be called after receiving a connection from a peer
    ///
    /// When making a connection using [Self::connect] there's no need to call this because the connect method already configures the peer.
    #[instrument(level = Level::DEBUG, skip(self))]
    pub fn configure_peer(
        &mut self,
        id: ControlPeerId,
        config: ControlPeerConfig,
    ) -> Result<(), ControlError> {
        if self.host.peer(id.0).is_none() {
            return Err(ControlError::Enet(EnetError::PeerNotFound));
        }

        self.peer_data.insert(
            id,
            PeerData {
                send_sequence_number: 0,
                config,
            },
        );

        Ok(())
    }

    #[instrument(level = Level::TRACE, skip(self))]
    pub fn send(
        &mut self,
        id: ControlPeerId,
        channel_id: EnetChannel,
        kind: PacketKind,
        packet: ControlPacket,
    ) -> Result<(), ControlError> {
        // Avoid spam from some packets
        if matches!(
            packet,
            ControlPacket::PeriodicPing
                | ControlPacket::FrameFec { .. }
                | ControlPacket::MouseMoveAbsolute { .. }
                | ControlPacket::MouseMoveRelative { .. }
                | ControlPacket::MouseButton { .. }
                | ControlPacket::MouseHorizontalScroll { .. }
                | ControlPacket::MouseScroll { .. }
                | ControlPacket::ControllerBattery { .. }
                | ControlPacket::ControllerState { .. }
        ) {
            trace!(packet = ?packet, "sending packet");
        } else {
            debug!(packet = ?packet, "sending packet");
        }

        // Firstly see if the peer exists
        let Some(peer) = self.host.peer_mut(id.0) else {
            return Err(ControlError::Enet(EnetError::PeerNotFound));
        };

        // Then if we've got a config
        let data = self
            .peer_data
            .get_mut(&id)
            .ok_or(ControlError::NotConfigured)?;

        if packet.ty().direction() != data.config.role.outgoing_direction() {
            warn!(
                expected_direction = ?data.config.role.outgoing_direction(),
                got_direction = ?packet.ty().direction(),
                packet = ?packet,
                "tried sending a packet into the wrong direction"
            );
            return Err(ControlError::PacketNotSupported(ControlPacketNotSupported));
        }

        let mut unencrypted_buffer = [0; ControlPacket::MAX_SIZE];

        let len = packet.serialize(&data.config.packets, &mut unencrypted_buffer)?;

        let mut encrypted_buffer = [0; EncryptedControlHeader::SIZE + ControlPacket::MAX_SIZE];
        let buffer = if let Some((method, aes_key)) = data.config.encryption {
            let encrypted_len = match data.config.role {
                ControlPeerRole::Client => encrypt_serverbound_control_packet_into(
                    &self.crypto_backend,
                    method,
                    aes_key,
                    data.send_sequence_number,
                    &unencrypted_buffer[0..len],
                    &mut encrypted_buffer,
                )?,
                ControlPeerRole::Server => encrypt_clientbound_control_packet_into(
                    &self.crypto_backend,
                    method,
                    aes_key,
                    data.send_sequence_number,
                    &unencrypted_buffer[0..len],
                    &mut encrypted_buffer,
                )?,
            };

            // increase sequence number
            data.send_sequence_number = data.send_sequence_number.wrapping_add(1);

            &encrypted_buffer[0..encrypted_len]
        } else {
            &unencrypted_buffer[0..len]
        };

        peer.send(channel_id.0, &Packet::new(buffer, kind))
            .map_err(EnetError::from)?;

        self.host.service();

        Ok(())
    }

    pub fn configured_peers(&self) -> impl Iterator<Item = ControlPeerId> {
        self.peer_data.keys().copied()
    }

    pub fn peer_estimated_rtt(&self, peer: ControlPeerId) -> Option<EstimatedRttInfo> {
        let peer = self.host.peer(peer.0)?;

        Some(EstimatedRttInfo {
            rtt: peer.round_trip_time(),
            rtt_variance: peer.round_trip_time_variance(),
        })
    }

    #[instrument(level = Level::DEBUG, skip(self))]
    pub fn disconnect(&mut self, id: ControlPeerId, data: u32) -> Result<(), ControlError> {
        self.host.disconnect(id.0, data)?;

        self.host.service();
        Ok(())
    }

    #[instrument(level = Level::DEBUG, skip(self))]
    pub fn disconnect_now(&mut self, id: ControlPeerId, data: u32) -> Result<(), ControlError> {
        self.host.disconnect_now(id.0, data)?;

        self.host.service();
        Ok(())
    }

    fn handle_events(&mut self) -> Result<(), ControlError> {
        while let Some(event) = self.host.poll_event() {
            trace!(event = ?event, "enet event");

            let event = match event {
                EnetEvent::Connect { peer, data } => ControlHostEvent::Connected {
                    id: ControlPeerId(peer),
                    sunshine_connect_data: if data == 0 { None } else { Some(data) },
                },
                EnetEvent::Receive {
                    peer,
                    channel_id,
                    data,
                } => {
                    let id = ControlPeerId(peer);

                    let Some(peer_data) = self.peer_data.get(&id) else {
                        warn!(peer_id = ?id, "dropping packet because the peer was not configured!");
                        continue;
                    };

                    let mut unencrypted_buffer = [0; ControlPacket::MAX_SIZE];
                    let buffer = if let Some((method, aes_key)) = peer_data.config.encryption {
                        // TODO: don't error when there's some error in receiving
                        let unencrypted_len = match peer_data.config.role {
                            ControlPeerRole::Client => decrypt_clientbound_control_packet_into(
                                &self.crypto_backend,
                                method,
                                aes_key,
                                &data,
                                &mut unencrypted_buffer,
                            )?,
                            ControlPeerRole::Server => decrypt_serverbound_control_packet_into(
                                &self.crypto_backend,
                                method,
                                aes_key,
                                &data,
                                &mut unencrypted_buffer,
                            )?,
                        };

                        &unencrypted_buffer[0..unencrypted_len]
                    } else {
                        &data
                    };

                    let packet_direction = match peer_data.config.role {
                        ControlPeerRole::Client => PacketDirection::ClientBound,
                        ControlPeerRole::Server => PacketDirection::ServerBound,
                    };

                    let Some(packet) = ControlPacket::deserialize(
                        packet_direction,
                        &peer_data.config.packets,
                        buffer,
                    ) else {
                        warn!(peer_id = ?id, peer_data = ?peer_data, unencrypted_packet = ?buffer, "received unknown packet");
                        continue;
                    };

                    trace!(peer_id = ?id, packet = ?packet, "received packet");

                    ControlHostEvent::Receive {
                        id,
                        channel_id,
                        packet,
                    }
                }
                EnetEvent::Disconnect { peer, data: _ } => {
                    let id = ControlPeerId(peer);

                    self.peer_data.remove(&id);

                    ControlHostEvent::Disconnected {
                        id: ControlPeerId(peer),
                    }
                }
            };

            self.events.push_back(event);
        }

        Ok(())
    }

    /// If this object can be discarded
    /// - Checks if there's still any connection that is currently happening
    pub fn can_discard(&mut self) -> bool {
        self.host
            .enet
            .peers()
            .filter(|peer| {
                matches!(
                    peer.state(),
                    PeerState::AcknowledgingConnect
                        | PeerState::AcknowledgingDisconnect
                        | PeerState::Connected
                        | PeerState::Connecting
                        | PeerState::ConnectionPending
                        | PeerState::ConnectionSucceeded
                        | PeerState::DisconnectLater
                        | PeerState::Disconnecting
                )
            })
            .count()
            == 0
    }
}

impl UdpStream for ControlHost {
    type Error = ControlError;

    type Event = ControlHostEvent;

    fn pending_send(&self) -> Option<(SocketAddr, &[u8])> {
        self.host.pending_send()
    }
    fn consume_send(&mut self) {
        self.host.consume_send();
    }

    fn poll_timeout(&self) -> Option<Instant> {
        Some(self.host.poll_timeout())
    }

    fn poll_event(&mut self) -> Option<ControlHostEvent> {
        self.events.pop_front()
    }

    fn handle_receive(
        &mut self,
        now: Instant,
        addr: SocketAddr,
        data: &[u8],
    ) -> Result<(), ControlError> {
        self.host.handle_receive(now, addr, data);

        self.handle_events()?;
        Ok(())
    }

    fn handle_timeout(&mut self, now: Instant) -> Result<(), ControlError> {
        self.host.handle_timeout(now);

        self.handle_events()?;
        Ok(())
    }
}
