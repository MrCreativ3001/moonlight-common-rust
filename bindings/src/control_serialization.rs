use moonlight_common::{
    ServerVersion,
    stream::proto::control::packet::{
        ControlPacket as ControlPacket2, ControlPacketConfig, PacketDirection, RawControlPacketType,
    },
};
use uniffi::{custom_type, export, remote};

use crate::control_packet::ControlPacket;

custom_type!(RawControlPacketType, u16, {
    remote,
    lower: |ty| ty.0,
    try_lift: |num| Ok(RawControlPacketType(num)),
});

#[remote(Record)]
pub struct ControlPacketConfig {
    pub server_version: ServerVersion,
    pub periodic_ping: Option<RawControlPacketType>,
    pub request_idr: RawControlPacketType,
    pub start_b: RawControlPacketType,
    pub invalidate_reference_frames: RawControlPacketType,
    pub long_term_reference_frame_acknowledgement: Option<RawControlPacketType>,
    pub loss_stats: RawControlPacketType,
    pub frame_stats: RawControlPacketType,
    pub rumble_data: Option<RawControlPacketType>,
    pub server_termination: Option<RawControlPacketType>,
    pub hdr_mode: Option<RawControlPacketType>,
    pub input_data: RawControlPacketType,
    pub frame_fec: Option<RawControlPacketType>,
    pub rumble_triggers: Option<RawControlPacketType>,
    pub set_motion_event: Option<RawControlPacketType>,
    pub set_rgb_led: Option<RawControlPacketType>,
    pub set_adaptive_triggers: Option<RawControlPacketType>,
}

#[export]
pub fn control_packet_config_new(
    server_version: ServerVersion,
    encrypted: bool,
) -> Option<ControlPacketConfig> {
    ControlPacketConfig::new(server_version, encrypted)
}

#[export]
pub fn control_packet_serialize(
    config: &ControlPacketConfig,
    packet: ControlPacket,
) -> Option<Vec<u8>> {
    let packet: ControlPacket2 = packet.into();

    let mut buffer = [0; _];
    let len = packet.serialize(config, &mut buffer).ok()?;

    Some(buffer[0..len].to_vec())
}

#[remote(Enum)]
pub enum PacketDirection {
    ClientBound,
    ServerBound,
}

#[export]
pub fn control_packet_deserialize(
    config: &ControlPacketConfig,
    packet_direction: PacketDirection,
    payload: Vec<u8>,
) -> Option<ControlPacket> {
    let packet = ControlPacket2::deserialize(packet_direction, config, &payload)?;

    Some(packet.into())
}
