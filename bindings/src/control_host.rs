use std::{
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::Duration,
};

use moonlight_common::{
    ServerVersion,
    crypto::rustcrypto::RustCryptoBackend,
    stream::{
        control::EstimatedRttInfo,
        proto::{
            Instant,
            control::{
                packet::{ControlPacketConfig, EnetChannel},
                peer::{
                    ControlConnectConfig as ControlConnectConfig2, ControlError as ControlError2,
                    ControlHost as ControlHost2, ControlHostConfig as ControlHostConfig2,
                    ControlPeerConfig as ControlPeerConfig2, ControlPeerId, ControlPeerRole,
                    PacketKind,
                },
            },
        },
    },
};
use uniffi::{Error, Object, Record, custom_type, deps::anyhow::Error, export, remote};

use crate::{MoonlightError, control_packet::ControlPacket, control_stream::ControlEncryption};

#[derive(Debug, thiserror::Error, Error)]
pub enum ControlError {
    #[error("this version of the protocol is not supported: {0}")]
    VersionNotSupported(ServerVersion),
    #[error("the control stream hasn't successfully connected yet")]
    NotConnected,
    #[error("packet not supported")]
    PacketNotSupported,
    #[error("the apollo permissions list doesn't allow this action")]
    ApolloPermissionDenied,
    #[error("{0}")]
    Other(MoonlightError),
}

impl From<ControlError2> for ControlError {
    fn from(value: ControlError2) -> Self {
        match value {
            ControlError2::VersionNotSupported(server_version) => {
                Self::VersionNotSupported(server_version)
            }
            ControlError2::NotConnected => Self::NotConnected,
            ControlError2::PacketNotSupported(_) => Self::PacketNotSupported,
            ControlError2::ApolloPermissionDenied => Self::ApolloPermissionDenied,
            err => Self::Other(err.into()),
        }
    }
}

custom_type!(ControlPeerId, u32, {
    remote,
    // Lowering the Rust type
    lower: |peer_id| usize::from(peer_id) as u32,
    // Lifting the foreign type
    try_lift: |num| Result::<_, Error>::Ok(ControlPeerId::from(num as usize)),
});

#[remote(Enum)]
pub enum PacketKind {
    Unreliable,
    Sequenced,
    AlwaysUnreliable,
    AlwaysSequenced,
    Reliable,
}

#[derive(Debug, Record)]
pub struct ControlHostConfig {
    pub peer_count: u32,
    pub peer_channel_count: u32,
}
impl From<ControlHostConfig> for ControlHostConfig2 {
    fn from(value: ControlHostConfig) -> Self {
        Self {
            peer_count: value.peer_count as usize,
            peer_channel_count: value.peer_channel_count as usize,
        }
    }
}

#[remote(Enum)]
pub enum ControlPeerRole {
    Client,
    Server,
}

#[derive(Debug, Record)]
pub struct ControlPeerConfig {
    pub role: ControlPeerRole,
    pub encryption: Option<ControlEncryption>,
    pub packets: ControlPacketConfig,
}

impl From<ControlPeerConfig> for ControlPeerConfig2 {
    fn from(value: ControlPeerConfig) -> Self {
        Self {
            role: value.role,
            encryption: value
                .encryption
                .map(|encryption| (encryption.method, encryption.aes_key)),
            packets: value.packets,
        }
    }
}

#[derive(Debug, Record)]
pub struct ControlConnectConfig {
    pub channel_count: u32,
    pub sunshine_connect_data: Option<u32>,
    pub config: ControlPeerConfig,
}

impl From<ControlConnectConfig> for ControlConnectConfig2 {
    fn from(value: ControlConnectConfig) -> Self {
        Self {
            channel_count: value.channel_count as usize,
            sunshine_connect_data: value.sunshine_connect_data,
            config: value.config.into(),
        }
    }
}

custom_type!(EnetChannel, u8, {
    remote,
    // Lowering the Rust type
    lower: |channel| channel.0,
    // Lifting the foreign type
    try_lift: |num| Result::<_, Error>::Ok(EnetChannel(num)),
});

#[derive(Object)]
pub struct ControlHost {
    inner: Mutex<ControlHost2>,
}

#[export]
impl ControlHost {
    #[uniffi::constructor]
    pub fn new(now: Instant, config: ControlHostConfig) -> Result<Arc<Self>, ControlError> {
        let this = Arc::new(Self {
            inner: Mutex::new(ControlHost2::new(
                now,
                config.into(),
                Arc::new(RustCryptoBackend),
            )?),
        });

        Ok(this)
    }

    pub fn configure_peer(
        &self,
        id: ControlPeerId,
        config: ControlPeerConfig,
    ) -> Result<(), ControlError> {
        let mut inner = self.inner.lock().expect("lock ControlHost");
        inner.configure_peer(id, config.into())?;
        Ok(())
    }

    pub fn connect(
        &self,
        addr: SocketAddr,
        config: ControlConnectConfig,
    ) -> Result<ControlPeerId, ControlError> {
        let mut inner = self.inner.lock().expect("lock ControlHost");
        let id = inner.connect(addr, config.into())?;
        Ok(id)
    }

    pub fn send(
        &self,
        id: ControlPeerId,
        channel_id: EnetChannel,
        kind: PacketKind,
        packet: ControlPacket,
    ) -> Result<(), ControlError> {
        let mut inner = self.inner.lock().expect("lock ControlHost");
        inner.send(id, channel_id, kind, packet.into())?;
        Ok(())
    }

    pub fn configured_peers(&self) -> Vec<ControlPeerId> {
        let inner = self.inner.lock().expect("lock ControlHost");
        inner.configured_peers().collect()
    }

    pub fn peer_estimated_rtt(&self, peer: ControlPeerId) -> Option<EstimatedRttInfo> {
        let inner = self.inner.lock().expect("lock ControlHost");
        inner.peer_estimated_rtt(peer)
    }

    pub fn set_peer_ping_interval(&self, peer: ControlPeerId, ping_interval: Duration) -> bool {
        let mut inner = self.inner.lock().expect("lock ControlHost");
        inner.set_peer_ping_interval(peer, ping_interval)
    }

    pub fn set_peer_timeout(
        &self,
        peer: ControlPeerId,
        limit: Duration,
        minimum: Duration,
        maximum: Duration,
    ) -> bool {
        let mut inner = self.inner.lock().expect("lock ControlHost");
        inner.set_peer_timeout(peer, limit, minimum, maximum)
    }

    pub fn disconnect(&self, id: ControlPeerId, data: u32) -> Result<(), ControlError> {
        let mut inner = self.inner.lock().expect("lock ControlHost");
        inner.disconnect(id, data)?;
        Ok(())
    }

    pub fn disconnect_now(&self, id: ControlPeerId, data: u32) -> Result<(), ControlError> {
        let mut inner = self.inner.lock().expect("lock ControlHost");
        inner.disconnect_now(id, data)?;
        Ok(())
    }
}
