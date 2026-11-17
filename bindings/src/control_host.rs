use std::{
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::Duration,
};

use moonlight_common::{
    crypto::rustcrypto::RustCryptoBackend,
    stream::{
        AesKey,
        control::EstimatedRttInfo,
        proto::{
            Instant,
            control::{
                packet::{ControlPacketConfig, EnetChannel},
                peer::{
                    ControlConnectConfig as ControlConnectConfig2, ControlEncryptionMethod,
                    ControlHost as ControlHost2, ControlHostConfig as ControlHostConfig2,
                    ControlHostEvent as ControlHostEvent2, ControlPeerConfig as ControlPeerConfig2,
                    ControlPeerId, ControlPeerRole, PacketKind,
                },
            },
            runtime::{Receive, UdpStream},
        },
    },
};
use uniffi::{Enum, Object, Record, custom_type, deps::anyhow::Error, export, remote};

use crate::{MoonlightError, UdpTransmit, control_packet::ControlPacket};

#[remote(Enum)]
pub enum ControlEncryptionMethod {
    Nvidia,
    Sunshine,
}

#[derive(Debug, Record)]
pub struct ControlEncryption {
    pub method: ControlEncryptionMethod,
    pub aes_key: AesKey,
}

#[remote(Record)]
pub struct EstimatedRttInfo {
    pub rtt: Duration,
    pub rtt_variance: Duration,
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

#[derive(Debug, Enum)]
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

impl From<ControlHostEvent2> for ControlHostEvent {
    fn from(value: ControlHostEvent2) -> Self {
        match value {
            ControlHostEvent2::Connected {
                id,
                sunshine_connect_data,
            } => Self::Connected {
                id,
                sunshine_connect_data,
            },
            ControlHostEvent2::Receive {
                id,
                channel_id,
                packet,
            } => Self::Receive {
                id,
                channel_id,
                packet: packet.into(),
            },
            ControlHostEvent2::Disconnected { id } => Self::Disconnected { id },
        }
    }
}

#[derive(Object)]
pub struct ControlHost {
    inner: Mutex<ControlHost2>,
}

#[export]
impl ControlHost {
    #[uniffi::constructor]
    pub fn new(now: Instant, config: ControlHostConfig) -> Result<Arc<Self>, MoonlightError> {
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
    ) -> Result<(), MoonlightError> {
        let mut inner = self.inner.lock().expect("lock ControlHost");
        inner.configure_peer(id, config.into())?;
        Ok(())
    }

    pub fn connect(
        &self,
        addr: SocketAddr,
        config: ControlConnectConfig,
    ) -> Result<ControlPeerId, MoonlightError> {
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
    ) -> Result<(), MoonlightError> {
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

    pub fn disconnect(&self, id: ControlPeerId, data: u32) -> Result<(), MoonlightError> {
        let mut inner = self.inner.lock().expect("lock ControlHost");
        inner.disconnect(id, data)?;
        Ok(())
    }

    pub fn disconnect_now(&self, id: ControlPeerId, data: u32) -> Result<(), MoonlightError> {
        let mut inner = self.inner.lock().expect("lock ControlHost");
        inner.disconnect_now(id, data)?;
        Ok(())
    }

    // -- Sans IO

    pub fn poll_event(&self) -> Option<ControlHostEvent> {
        let mut inner = self.inner.lock().expect("lock ControlHost");
        let event = inner.poll_event()?;
        Some(event.into())
    }

    pub fn poll_timeout(&self) -> Option<Instant> {
        let inner = self.inner.lock().expect("lock ControlHost");
        inner.poll_timeout()
    }

    pub fn poll_packet(&self) -> Option<UdpTransmit> {
        let mut inner = self.inner.lock().expect("lock ControlHost");

        inner.poll_transmit().map(UdpTransmit::from)
    }

    pub fn handle_receive(
        &self,
        now: Instant,
        addr: SocketAddr,
        contents: Vec<u8>,
    ) -> Result<(), MoonlightError> {
        let mut inner = self.inner.lock().expect("lock ControlHost");
        inner.handle_receive(
            now,
            Receive {
                source: addr,
                data: &contents,
            },
        )?;
        Ok(())
    }

    pub fn handle_timeout(&self, now: Instant) -> Result<(), MoonlightError> {
        let mut inner = self.inner.lock().expect("lock ControlHost");
        inner.handle_timeout(now)?;
        Ok(())
    }
}
