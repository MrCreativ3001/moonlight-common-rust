use uniffi::{Enum, custom_type};

use moonlight_common::stream::proto::control::{
    ControlMessage, ControlMessageInner as ControlMessageInner2,
};

use crate::control_packet::ControlPacket;

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
