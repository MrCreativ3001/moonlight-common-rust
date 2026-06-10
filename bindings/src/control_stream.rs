use std::{
    net::SocketAddr,
    sync::{Arc, Mutex},
};

use uniffi::{Enum, Error, Object, Record, custom_type, export, remote};

use moonlight_common::{
    ServerVersion,
    crypto::rustcrypto::RustCryptoBackend,
    stream::{
        AesKey,
        proto::{
            Instant,
            control::{
                ControlMessage, ControlMessageInner as ControlMessageInner2,
                ControlStream as ControlStream2, ControlStreamConfig as ControlStreamConfig2,
                ControlStreamEvent as ControlStreamEvent2,
                ControlStreamInput as ControlStreamInput2,
                ControlStreamOutput as ControlStreamOutput2,
                peer::{ControlEncryptionMethod, ControlError as ControlError2, ControlHostAction},
            },
        },
    },
};

use crate::{MoonlightError, control_packet::ControlPacket, input_batcher::ClientInputEvent};

custom_type!(ControlMessage, ControlMessageInner, {
    remote,
    lower: |msg| msg.0.into(),
    try_lift: |inner| Ok(ControlMessage(inner.into())),
});

#[derive(Debug, Enum)]
pub enum ControlMessageInner {
    SendPacket { packet: ControlPacket, force: bool },
}

impl From<ControlMessageInner2> for ControlMessageInner {
    fn from(value: ControlMessageInner2) -> Self {
        match value {
            ControlMessageInner2::SendPacket { packet, force } => Self::SendPacket {
                packet: packet.into(),
                force,
            },
        }
    }
}
impl From<ControlMessageInner> for ControlMessageInner2 {
    fn from(value: ControlMessageInner) -> Self {
        match value {
            ControlMessageInner::SendPacket { packet, force } => Self::SendPacket {
                packet: packet.into(),
                force,
            },
        }
    }
}

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
pub enum ControlStreamInput {
    Timeout(Instant),
    Message {
        now: Instant,
        message: ControlMessage,
    },
    Receive {
        now: Instant,
        addr: SocketAddr,
        data: Vec<u8>,
    },
}

#[derive(Debug, Enum)]
pub enum ControlStreamEvent {
    Connect,
    Packet(ControlPacket),
    Disconnect,
}

#[derive(Debug, Enum)]
pub enum ControlStreamOutput {
    Timeout(Instant),
    Send { addr: SocketAddr, data: Vec<u8> },
    Event(ControlStreamEvent),
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

    pub fn handle_input(&self, input: ControlStreamInput) -> Result<(), MoonlightError> {
        let input = match input {
            ControlStreamInput::Receive {
                now,
                addr,
                ref data,
            } => ControlStreamInput2::Receive { now, addr, data },
            ControlStreamInput::Timeout(instant) => ControlStreamInput2::Timeout(instant),
            ControlStreamInput::Message { now, message } => {
                ControlStreamInput2::Message { now, message }
            }
        };

        let mut inner = self.inner.lock().expect("lock ControlStream");
        inner.handle_input(input)?;
        Ok(())
    }

    pub fn poll_output(&self) -> Result<ControlStreamOutput, MoonlightError> {
        let mut inner = self.inner.lock().expect("lock ControlStream");
        let output = inner.poll_output()?;

        let output = match output {
            ControlStreamOutput2::Action(ControlHostAction::SendUdp { addr, data }) => {
                ControlStreamOutput::Send { addr, data }
            }
            ControlStreamOutput2::Action(ControlHostAction::Timeout(timeout)) => {
                ControlStreamOutput::Timeout(timeout)
            }
            ControlStreamOutput2::Event(ControlStreamEvent2::Connect) => {
                ControlStreamOutput::Event(ControlStreamEvent::Connect)
            }
            ControlStreamOutput2::Event(ControlStreamEvent2::Packet(packet)) => {
                ControlStreamOutput::Event(ControlStreamEvent::Packet(packet.into()))
            }
            ControlStreamOutput2::Event(ControlStreamEvent2::Disconnect) => {
                ControlStreamOutput::Event(ControlStreamEvent::Disconnect)
            }
        };

        Ok(output)
    }
}
