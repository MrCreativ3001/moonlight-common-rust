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
        control::{
            ControllerType, KeyAction, KeyCode, MouseButton, MouseButtonAction, ToolType,
            TouchEventType,
        },
        proto::{
            Instant,
            control::{
                ClientInputEvent as ClientInputEvent2, ControlMessage,
                ControlMessageInner as ControlMessageInner2, ControlStream as ControlStream2,
                ControlStreamConfig as ControlStreamConfig2,
                ControlStreamEvent as ControlStreamEvent2,
                ControlStreamInput as ControlStreamInput2,
                ControlStreamOutput as ControlStreamOutput2,
                peer::{ControlEncryptionMethod, ControlError as ControlError2, ControlHostAction},
            },
        },
    },
};

use crate::control_packet::{
    ControlPacket, ControllerButtons, ControllerCapabilities, KeyFlags, KeyModifiers, PenButtons,
};

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
pub enum ControlError {
    #[error("this version of the protocol is not supported: {0}")]
    VersionNotSupported(ServerVersion),
    #[error("enet: {reason}")]
    Enet { reason: String },
    #[error("the control stream hasn't successfully connected yet")]
    NotConnected,
    #[error("the peer was not configured, but this is required to do this action")]
    NotConfigured,
    #[error("packet not supported")]
    PacketNotSupported,
    #[error("the apollo permissions list doesn't allow this action")]
    ApolloPermissionDenied,
    #[error("encryption: {reason}")]
    Encryption { reason: String },
}

impl From<ControlError2> for ControlError {
    fn from(value: ControlError2) -> Self {
        match value {
            ControlError2::VersionNotSupported(server_version) => {
                Self::VersionNotSupported(server_version)
            }
            ControlError2::Enet(err) => Self::Enet {
                reason: err.to_string(),
            },
            ControlError2::NotConnected => Self::NotConnected,
            ControlError2::NotConfigured => Self::NotConfigured,
            ControlError2::PacketNotSupported(_) => Self::PacketNotSupported,
            ControlError2::ApolloPermissionDenied => Self::ApolloPermissionDenied,
            ControlError2::Encryption(err) => Self::Encryption {
                reason: err.to_string(),
            },
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

#[derive(Debug, Enum)]
pub enum ClientInputEvent {
    Keyboard {
        action: KeyAction,
        flags: KeyFlags,
        key_code: KeyCode,
        modifiers: KeyModifiers,
    },
    MouseMoveRelative {
        delta_x: i16,
        delta_y: i16,
    },
    MouseMoveAbsolute {
        x: i16,
        y: i16,
        reference_width: i16,
        reference_height: i16,
    },
    MouseButton {
        action: MouseButtonAction,
        button: MouseButton,
    },
    MouseScrollVertical {
        scroll_y: i16,
    },
    MouseScrollHorizontal {
        scroll_x: i16,
    },
    ControllerConnect {
        controller_number: u8,
        ty: ControllerType,
        capabilities: ControllerCapabilities,
        supported_buttons: ControllerButtons,
    },
    ControllerState {
        controller_number: u8,
        pressed_buttons: ControllerButtons,
        left_trigger: f32,
        right_trigger: f32,
        left_stick_x: f32,
        left_stick_y: f32,
        right_stick_x: f32,
        right_stick_y: f32,
    },
    ControllerDisconnect {
        controller_number: u8,
    },
    Touch {
        event_type: TouchEventType,
        rotation: Option<u16>,
        pointer_id: u32,
        x: f32,
        y: f32,
        pressure_or_distance: f32,
        contact_area_minor: f32,
        contact_area_major: f32,
    },
    Pen {
        event_type: TouchEventType,
        tool_type: ToolType,
        buttons: PenButtons,
        x: f32,
        y: f32,
        pressure_or_distance: f32,
        rotation: Option<u16>,
        tilt: Option<u8>,
        contact_area_minor: f32,
        contact_area_major: f32,
    },
}

impl From<ClientInputEvent> for ClientInputEvent2 {
    fn from(value: ClientInputEvent) -> Self {
        match value {
            ClientInputEvent::Keyboard {
                action,
                flags,
                key_code,
                modifiers,
            } => ClientInputEvent2::Keyboard {
                action,
                flags: flags.into(),
                key_code,
                modifiers: modifiers.into(),
            },
            ClientInputEvent::MouseMoveRelative { delta_x, delta_y } => {
                ClientInputEvent2::MouseMoveRelative { delta_x, delta_y }
            }
            ClientInputEvent::MouseMoveAbsolute {
                x,
                y,
                reference_width,
                reference_height,
            } => ClientInputEvent2::MouseMoveAbsolute {
                x,
                y,
                reference_width,
                reference_height,
            },
            ClientInputEvent::MouseButton { action, button } => {
                ClientInputEvent2::MouseButton { action, button }
            }
            ClientInputEvent::MouseScrollVertical { scroll_y } => {
                ClientInputEvent2::MouseScrollVertical { scroll_y }
            }
            ClientInputEvent::MouseScrollHorizontal { scroll_x } => {
                ClientInputEvent2::MouseScrollHorizontal { scroll_x }
            }
            ClientInputEvent::ControllerConnect {
                controller_number,
                ty,
                capabilities,
                supported_buttons,
            } => ClientInputEvent2::ControllerConnect {
                controller_number,
                ty,
                capabilities: capabilities.into(),
                supported_buttons: supported_buttons.into(),
            },
            ClientInputEvent::ControllerState {
                controller_number,
                pressed_buttons,
                left_trigger,
                right_trigger,
                left_stick_x,
                left_stick_y,
                right_stick_x,
                right_stick_y,
            } => ClientInputEvent2::ControllerState {
                controller_number,
                pressed_buttons: pressed_buttons.into(),
                left_trigger,
                right_trigger,
                left_stick_x,
                left_stick_y,
                right_stick_x,
                right_stick_y,
            },
            ClientInputEvent::ControllerDisconnect { controller_number } => {
                ClientInputEvent2::ControllerDisconnect { controller_number }
            }
            ClientInputEvent::Touch {
                event_type,
                rotation,
                pointer_id,
                x,
                y,
                pressure_or_distance,
                contact_area_minor,
                contact_area_major,
            } => ClientInputEvent2::Touch {
                event_type,
                rotation,
                pointer_id,
                x,
                y,
                pressure_or_distance,
                contact_area_minor,
                contact_area_major,
            },
            ClientInputEvent::Pen {
                event_type,
                tool_type,
                buttons,
                x,
                y,
                pressure_or_distance,
                rotation,
                tilt,
                contact_area_minor,
                contact_area_major,
            } => ClientInputEvent2::Pen {
                event_type,
                tool_type,
                buttons: buttons.into(),
                x,
                y,
                pressure_or_distance,
                rotation,
                tilt,
                contact_area_minor,
                contact_area_major,
            },
        }
    }
}

#[derive(Debug, Object)]
pub struct ControlStream {
    inner: Mutex<ControlStream2>,
}

#[export]
impl ControlStream {
    #[uniffi::constructor]
    pub fn new(now: Instant, config: ControlStreamConfig) -> Result<Arc<Self>, ControlError> {
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

    pub fn batch_input(&self, input: ClientInputEvent) -> Result<(), ControlError> {
        let mut inner = self.inner.lock().expect("lock AudioStream");
        inner.batch_input(input.into())?;
        Ok(())
    }

    pub fn send_raw(&self, packet: ControlPacket) -> Result<(), ControlError> {
        let mut inner = self.inner.lock().expect("lock ControlStream");
        inner.send_raw(packet.into())?;
        Ok(())
    }

    pub fn disconnect(&self, disconnect_data: u32) -> Result<(), ControlError> {
        let mut inner = self.inner.lock().expect("lock ControlStream");
        inner.disconnect(disconnect_data)?;
        Ok(())
    }

    pub fn handle_input(&self, input: ControlStreamInput) -> Result<(), ControlError> {
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

    pub fn poll_output(&self) -> Result<ControlStreamOutput, ControlError> {
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
