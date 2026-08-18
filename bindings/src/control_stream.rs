use std::{
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::Duration,
};

use uniffi::{Enum, Error, Object, Record, export, remote};

use moonlight_common::{
    ServerVersion,
    crypto::rustcrypto::RustCryptoBackend,
    stream::{
        AesKey,
        control::EstimatedRttInfo,
        proto::{
            Instant,
            control::{
                ControlStream as ControlStream2, ControlStreamConfig as ControlStreamConfig2,
                ControlStreamEvent as ControlStreamEvent2,
                peer::{ControlEncryptionMethod, ControlError as ControlError2},
            },
            runtime::UdpStream,
        },
    },
};

use crate::{
    MoonlightError, UdpTransmit, control_packet::ControlPacket, input_batcher::ClientInputEvent,
};

#[derive(Debug, thiserror::Error, Error)]
pub enum ControlStreamError {
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

impl From<ControlError2> for ControlStreamError {
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

#[derive(Debug, Record)]
pub struct ControlStreamConfig {
    pub server_version: ServerVersion,
    pub addr: SocketAddr,
    pub sunshine_connect_data: Option<u32>,
    pub encryption: Option<ControlEncryption>,
    // TODO
    // pub apollo_permissions: Option<ApolloPermissions>,
}

#[derive(Debug, Enum)]
pub enum ControlStreamEvent {
    Connect,
    Packet(ControlPacket),
    Disconnect,
}

#[remote(Record)]
pub struct EstimatedRttInfo {
    pub rtt: Duration,
    pub rtt_variance: Duration,
}

#[derive(Debug, Object)]
pub struct ControlStream {
    inner: Mutex<ControlStream2>,
}

#[export]
impl ControlStream {
    #[uniffi::constructor]
    pub fn new(now: Instant, config: ControlStreamConfig) -> Result<Arc<Self>, ControlStreamError> {
        let this = Arc::new(Self {
            inner: Mutex::new(ControlStream2::new(
                now,
                ControlStreamConfig2 {
                    server_version: config.server_version,
                    addr: config.addr,
                    sunshine_connect_data: config.sunshine_connect_data,
                    encryption: config
                        .encryption
                        .map(|encryption| (encryption.method, encryption.aes_key)),
                    apollo_permissions: None,
                },
                Arc::new(RustCryptoBackend) as _,
            )?),
        });

        Ok(this)
    }

    pub fn estimated_rtt(&self) -> Result<EstimatedRttInfo, ControlStreamError> {
        let inner = self.inner.lock().expect("lock AudioStream");
        let rtt = inner.estimated_rtt()?;
        Ok(rtt)
    }

    pub fn batch_input(&self, input: ClientInputEvent) -> Result<(), ControlStreamError> {
        let mut inner = self.inner.lock().expect("lock AudioStream");
        inner.batch_input(input.into())?;
        Ok(())
    }

    pub fn send_raw(&self, packet: ControlPacket) -> Result<(), ControlStreamError> {
        let mut inner = self.inner.lock().expect("lock ControlStream");
        inner.send_raw(packet.into())?;
        Ok(())
    }

    pub fn disconnect(&self, disconnect_data: u32) -> Result<(), ControlStreamError> {
        let mut inner = self.inner.lock().expect("lock ControlStream");
        inner.disconnect(disconnect_data)?;
        Ok(())
    }

    pub fn can_discard(&self) -> bool {
        let mut inner = self.inner.lock().expect("lock ControlStream");
        inner.can_discard()
    }

    // -- Sans IO

    pub fn poll_event(&self) -> Option<ControlStreamEvent> {
        let mut inner = self.inner.lock().expect("lock ControlStream");
        Some(match inner.poll_event()? {
            ControlStreamEvent2::Connect => ControlStreamEvent::Connect,
            ControlStreamEvent2::Packet(packet) => ControlStreamEvent::Packet(packet.into()),
            ControlStreamEvent2::Disconnect => ControlStreamEvent::Disconnect,
        })
    }

    pub fn poll_timeout(&self) -> Option<Instant> {
        let inner = self.inner.lock().expect("lock ControlStream");
        inner.poll_timeout()
    }

    pub fn poll_packet(&self) -> Option<UdpTransmit> {
        let mut inner = self.inner.lock().expect("lock ControlStream");

        let result = inner.pending_send().map(|(addr, contents)| UdpTransmit {
            addr,
            contents: contents.to_vec(),
        });
        inner.consume_send();

        result
    }

    pub fn handle_receive(
        &self,
        now: Instant,
        addr: SocketAddr,
        contents: Vec<u8>,
    ) -> Result<(), MoonlightError> {
        let mut inner = self.inner.lock().expect("lock ControlStream");
        inner.handle_receive(now, addr, &contents)?;
        Ok(())
    }

    pub fn handle_timeout(&self, now: Instant) -> Result<(), MoonlightError> {
        let mut inner = self.inner.lock().expect("lock ControlStream");
        inner.handle_timeout(now)?;
        Ok(())
    }
}
