use std::{ops::Deref, time::Duration};

use num::FromPrimitive;
use rusty_enet::PacketKind;
use thiserror::Error;
use tracing::{Level, instrument, trace, warn};

use crate::{
    ServerVersion,
    stream::{
        control::{
            ActiveGamepads, BatteryState, ControllerButtons, ControllerCapabilities,
            ControllerType, KeyAction, KeyCode, KeyFlags, KeyModifiers, MotionType, MouseButton,
            MouseButtonAction, PenButtons, ToolType, TouchEventType,
        },
        video::{Primary, SunshineHdrMetadata},
    },
};

/// The server must be pinged every few milliseconds
///
/// References:
/// - Moonlight Interval: https://github.com/moonlight-stream/moonlight-common-c/blob/2a5a1f3e8a57cbbb316ed7dfff3a3965c2e77d25/src/ControlStream.c#L298
pub const PERIODIC_PING_INTERVAL: Duration = Duration::from_millis(100);
/// References:
/// - Moonlight Version Check: https://github.com/moonlight-stream/moonlight-common-c/blob/2a5a1f3e8a57cbbb316ed7dfff3a3965c2e77d25/src/ControlStream.c#L354
pub const PERIODIC_PING_VERSION: ServerVersion = ServerVersion::new(7, 1, 415, 0);

#[derive(Debug, Error)]
#[error("this packet is not supported on this version of moonlight")]
pub struct ControlPacketNotSupported;

/// Its possible to send control messages via tcp on very old versions: AppVersionQuad[0] < 5
/// - Create: https://github.com/moonlight-stream/moonlight-common-c/blob/435bc6a5a4852c90cfb037de1378c0334ed36d8e/src/ControlStream.c#L1784-L1793
/// - https://github.com/moonlight-stream/moonlight-common-c/blob/435bc6a5a4852c90cfb037de1378c0334ed36d8e/src/ControlStream.c#L825-L832
/// - https://github.com/moonlight-stream/moonlight-common-c/blob/435bc6a5a4852c90cfb037de1378c0334ed36d8e/src/ControlStream.c#L797-L820
pub struct ControlHeaderTcp {
    /// This seems to equal ControlHeaderV1.type
    pub ty: u16,
    /// The len of the packet, because tcp is streamed
    pub len: u16,
}
impl ControlHeaderTcp {
    pub const SIZE: usize = 4;

    pub fn deserialize(buffer: &[u8; Self::SIZE]) -> Self {
        let ty = u16::from_be_bytes([buffer[0], buffer[1]]);
        let len = u16::from_be_bytes([buffer[2], buffer[3]]);

        Self { ty, len }
    }

    pub fn serialize(&self, buffer: &mut [u8; Self::SIZE]) {
        buffer[0..2].copy_from_slice(&self.ty.to_be_bytes());
        buffer[2..4].copy_from_slice(&self.len.to_be_bytes());
    }
}

/// V1 Control Header:
/// - Definition: https://github.com/moonlight-stream/moonlight-common-c/blob/435bc6a5a4852c90cfb037de1378c0334ed36d8e/src/ControlStream.c#L16-L18
///
/// Used when message is not encrypted (default)
pub struct ControlHeaderV1 {
    pub ty: u16,
}

impl ControlHeaderV1 {
    pub const SIZE: usize = 2;

    pub fn deserialize(buffer: &[u8; Self::SIZE]) -> Self {
        let ty = u16::from_be_bytes([buffer[0], buffer[1]]);

        Self { ty }
    }
    pub fn serialize(&mut self, buffer: &mut [u8; Self::SIZE]) {
        buffer[0..2].copy_from_slice(&self.ty.to_be_bytes());
    }
}

/// V2 Control Header:
/// - Definition: https://github.com/moonlight-stream/moonlight-common-c/blob/435bc6a5a4852c90cfb037de1378c0334ed36d8e/src/ControlStream.c#L20-L23
///
/// The header of the decrypted payload which follows after the EncryptedControlHeader
pub struct ControlHeaderV2 {
    pub ty: u16,
    pub len: u16,
}

impl ControlHeaderV2 {
    pub const SIZE: usize = 4;

    pub fn deserialize(buffer: &[u8; Self::SIZE]) -> Self {
        let ty = u16::from_be_bytes([buffer[0], buffer[1]]);
        let len = u16::from_be_bytes([buffer[2], buffer[3]]);

        Self { ty, len }
    }

    pub fn serialize(&self, buffer: &mut [u8; Self::SIZE]) {
        buffer[0..2].copy_from_slice(&self.ty.to_be_bytes());
        buffer[2..4].copy_from_slice(&self.len.to_be_bytes());
    }
}

pub const ENCRYPTED_CONTROL_PACKET_AES_GCM_TAG_LENGTH: usize = 16;

/// References:
/// - Moonlight: https://github.com/moonlight-stream/moonlight-common-c/blob/62687809b1f7410c3db4be2527503a54ae408d70/src/ControlStream.c#L1222
pub const ENCRYPTED_CONTROL_PACKET_TYPE: u16 = 0x0001;

/// Encrypted Control Header:
///
/// Encryption requires version APP_VERSION_AT_LEAST(7, 1, 431):
///
/// - Version: https://github.com/moonlight-stream/moonlight-common-c/blob/435bc6a5a4852c90cfb037de1378c0334ed36d8e/src/ControlStream.c#L308
/// - Definition:
///   - https://games-on-whales.github.io/wolf/stable/protocols/control-specs.html#_encrypted_packet_format
///   - https://github.com/moonlight-stream/moonlight-common-c/blob/435bc6a5a4852c90cfb037de1378c0334ed36d8e/src/ControlStream.c#L25-L32
#[derive(Debug, PartialEq)]
pub struct EncryptedControlHeader {
    /// The type of message, fixed at 0x0001 for this type of packet
    pub ty: u16,
    /// The size of the rest of the message in bytes (Seq + TAG + Payload)
    pub len: u16,
    /// Monotonically increasing sequence number (used as IV for AES-GCM)
    pub sequence_number: u32,
    /// The AES GCM TAG
    pub tag: [u8; ENCRYPTED_CONTROL_PACKET_AES_GCM_TAG_LENGTH],
}

impl EncryptedControlHeader {
    pub const SIZE: usize = 24;

    pub fn deserialize(buffer: &[u8; Self::SIZE]) -> Self {
        let ty = u16::from_le_bytes([buffer[0], buffer[1]]);
        let len = u16::from_le_bytes([buffer[2], buffer[3]]);
        let sequence_number = u32::from_le_bytes([buffer[4], buffer[5], buffer[6], buffer[7]]);

        let mut tag = [0; 16];
        tag.copy_from_slice(&buffer[8..24]);

        Self {
            ty,
            len,
            sequence_number,
            tag,
        }
    }

    pub fn serialize(&self, buffer: &mut [u8; Self::SIZE]) {
        buffer[0..2].copy_from_slice(&self.ty.to_le_bytes());
        buffer[2..4].copy_from_slice(&self.len.to_le_bytes());
        buffer[4..8].copy_from_slice(&self.sequence_number.to_le_bytes());
        buffer[8..24].copy_from_slice(&self.tag);
    }

    pub fn len_with_payload_size(payload_size: usize) -> usize {
        4 + ENCRYPTED_CONTROL_PACKET_AES_GCM_TAG_LENGTH + payload_size
    }
    /// The size with the sequence_number and tag removed.
    /// Will also check bounds.
    pub fn payload_size(&self) -> Option<u16> {
        self.len
            .checked_sub((4 + ENCRYPTED_CONTROL_PACKET_AES_GCM_TAG_LENGTH) as u16)
    }
}

/// References:
/// - https://github.com/moonlight-stream/moonlight-common-c/blob/7b026e77be62175104640e7e722b758df6d3d0d7/src/Limelight-internal.h#L56-L66
#[derive(Debug, Clone, Copy)]
pub struct EnetChannel(pub u8);

impl EnetChannel {
    pub const CHANNEL_GENERIC: EnetChannel = EnetChannel(0x00);
    /// IDR and reference frame invalidation requests
    pub const CHANNEL_URGENT: EnetChannel = EnetChannel(0x01);
    pub const CHANNEL_KEYBOARD: EnetChannel = EnetChannel(0x02);
    pub const CHANNEL_MOUSE: EnetChannel = EnetChannel(0x03);
    pub const CHANNEL_PEN: EnetChannel = EnetChannel(0x04);
    pub const CHANNEL_TOUCH: EnetChannel = EnetChannel(0x05);
    pub const CHANNEL_UTF8: EnetChannel = EnetChannel(0x06);
    /// 0x10 to 0x1F by controller index
    pub const CHANNEL_GAMEPAD_BASE: EnetChannel = EnetChannel(0x10);
    /// 0x20 to 0x2F by controller index
    pub const CHANNEL_SENSOR_BASE: EnetChannel = EnetChannel(0x20);
    pub const CHANNEL_COUNT: usize = 0x30;

    pub fn controller(id: u8) -> Option<EnetChannel> {
        if id < 16 {
            Some(Self(Self::CHANNEL_GAMEPAD_BASE.0 + id))
        } else {
            None
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PacketDirection {
    /// A packet that is send to the client.
    ClientBound,
    /// A packet that is send to the server.
    ServerBound,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RawControlPacketType(pub u16);

impl Deref for RawControlPacketType {
    type Target = u16;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// The version of all packets
// TODO: when / how is this used
#[derive(Debug)]
pub enum PacketVersion {
    V1,
    V2,
}

// Packets:
// - New values: https://games-on-whales.github.io/wolf/stable/protocols/control-specs.html
// - Old Value: https://github.com/moonlight-stream/moonlight-common-c/blob/435bc6a5a4852c90cfb037de1378c0334ed36d8e/src/ControlStream.c#L146-L216
#[derive(Debug)]
pub struct ControlPacketConfig {
    /// Required because some packets have version depedent data
    pub server_version: ServerVersion,
    /// The version of all packets
    pub version: PacketVersion,
    /// See also:
    /// - [ControlPacket::PeriodicPing]
    pub periodic_ping: Option<RawControlPacketType>,
    /// This seems to also equal StartA
    ///
    /// See also:
    /// - [ControlPacket::RequestIdr]
    pub request_idr: RawControlPacketType,
    ///
    /// See also:
    /// - [ControlPacket::StartB]
    pub start_b: RawControlPacketType,
    ///
    /// See also:
    /// - [ControlPacket::InvalidateReferenceFrames]
    pub invalidate_reference_frames: RawControlPacketType,
    ///
    /// See also:
    /// - [ControlPacket::LongTermReferenceFrameAcknowledgement]
    pub long_term_reference_frame_acknowledgement: Option<RawControlPacketType>,
    ///
    /// See also:
    /// - [ControlPacket::LossStats]
    pub loss_stats: RawControlPacketType,
    ///
    /// See also:
    /// - [ControlPacket::FrameStats]
    pub frame_stats: RawControlPacketType,
    ///
    /// See also:
    /// - [ControlPacket::RumbleData]
    pub rumble_data: Option<RawControlPacketType>,
    ///
    /// See also:
    /// - [ControlPacket::ServerTermination]
    pub server_termination: Option<RawControlPacketType>,
    ///
    /// See also:
    /// - [ControlPacket::HdrMode]
    pub hdr_mode: Option<RawControlPacketType>,
    /// An input packet.
    ///
    /// All input related packets share this.
    /// e.g.: [ControlPacket::MouseMoveRelative] or [ControlPacket::Keyboard]
    pub input_data: RawControlPacketType,
    /// Sunshine Extension
    ///
    /// See also:
    /// - [ControlPacket::FrameFec]
    pub frame_fec: Option<RawControlPacketType>,
    /// Sunshine Extension
    ///
    /// See also:
    /// - [ControlPacket::RumbleTriggers]
    pub rumble_triggers: Option<RawControlPacketType>,
    /// Sunshine Extension
    ///
    /// See also:
    /// - [ControlPacket::SetMotionEvent]
    pub set_motion_event: Option<RawControlPacketType>,
    /// Sunshine Extension
    ///
    /// See also:
    /// - [ControlPacket::SetRgbLed]
    pub set_rgb_led: Option<RawControlPacketType>,
    /// Sunshine Extension
    ///
    /// See also:
    /// - [ControlPacket::SetAdaptiveTriggers]
    pub set_adaptive_triggers: Option<RawControlPacketType>,
}

impl ControlPacketConfig {
    /// References:
    /// - moonlight common c https://github.com/moonlight-stream/moonlight-common-c/blob/435bc6a5a4852c90cfb037de1378c0334ed36d8e/src/ControlStream.c#L310-L341
    pub fn new(server_version: ServerVersion, encrypted: bool) -> Option<Self> {
        match server_version.major {
            5 => Some(Self {
                server_version,

                version: PacketVersion::V1,

                periodic_ping: None,
                request_idr: RawControlPacketType(0x0305),
                start_b: RawControlPacketType(0x0307),
                invalidate_reference_frames: RawControlPacketType(0x0301),
                long_term_reference_frame_acknowledgement: None,
                loss_stats: RawControlPacketType(0x0201),
                frame_stats: RawControlPacketType(0x0204),

                input_data: RawControlPacketType(0x0207),

                rumble_data: None,
                server_termination: None,
                hdr_mode: None,

                frame_fec: server_version
                    .is_sunshine_like()
                    .then_some(RawControlPacketType(0x5502)),

                rumble_triggers: None,
                set_motion_event: None,
                set_rgb_led: None,
                set_adaptive_triggers: None,
            }),

            7 if encrypted => Some(Self {
                server_version,

                version: PacketVersion::V1,

                periodic_ping: (server_version >= PERIODIC_PING_VERSION)
                    .then_some(RawControlPacketType(0x0200)),

                request_idr: RawControlPacketType(0x0302),
                start_b: RawControlPacketType(0x0307),
                invalidate_reference_frames: RawControlPacketType(0x0301),
                long_term_reference_frame_acknowledgement: Some(RawControlPacketType(0x0350)),
                loss_stats: RawControlPacketType(0x0201),
                frame_stats: RawControlPacketType(0x0204),

                input_data: RawControlPacketType(0x0206),
                rumble_data: Some(RawControlPacketType(0x010b)),
                server_termination: Some(RawControlPacketType(0x0109)),
                hdr_mode: Some(RawControlPacketType(0x010e)),

                frame_fec: server_version
                    .is_sunshine_like()
                    .then_some(RawControlPacketType(0x5502)),

                rumble_triggers: Some(RawControlPacketType(0x5500)),
                set_motion_event: Some(RawControlPacketType(0x5501)),
                set_rgb_led: Some(RawControlPacketType(0x5502)),
                set_adaptive_triggers: Some(RawControlPacketType(0x5503)),
            }),
            //
            6 | 7 => Some(Self {
                server_version,

                version: PacketVersion::V1,

                periodic_ping: (server_version >= PERIODIC_PING_VERSION)
                    .then_some(RawControlPacketType(0x0200)),

                request_idr: RawControlPacketType(0x0305),
                start_b: RawControlPacketType(0x0307),
                invalidate_reference_frames: RawControlPacketType(0x0301),
                long_term_reference_frame_acknowledgement: Some(RawControlPacketType(0x0350)),
                loss_stats: RawControlPacketType(0x0201),
                frame_stats: RawControlPacketType(0x0204),

                input_data: RawControlPacketType(0x0206),
                rumble_data: Some(RawControlPacketType(0x010b)),
                server_termination: Some(RawControlPacketType(0x0100)),
                hdr_mode: Some(RawControlPacketType(0x010e)),

                frame_fec: server_version
                    .is_sunshine_like()
                    .then_some(RawControlPacketType(0x5502)),

                rumble_triggers: None,
                set_motion_event: None,
                set_rgb_led: None,
                set_adaptive_triggers: None,
            }),
            _ => None,
        }
    }
}

// When adding new types add a test!
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlPacketType {
    PeriodicPing,
    RequestIdr,
    StartB,
    InvalidateReferenceFrames,
    LongTermReferenceFrameAcknowledgement,
    LossStats,
    FrameStats,
    ControllerRumbleData,
    Termination,
    HdrMode,
    InputData,
    FrameFec,
    ControllerRumbleTriggers,
    ControllerSetMotion,
    ControllerSetLed,
    SetAdaptiveTriggers,
}

impl ControlPacketType {
    pub fn direction(&self) -> PacketDirection {
        match self {
            Self::PeriodicPing => PacketDirection::ServerBound,
            Self::RequestIdr => PacketDirection::ServerBound,
            Self::StartB => PacketDirection::ServerBound,
            Self::InvalidateReferenceFrames => PacketDirection::ServerBound,
            Self::LongTermReferenceFrameAcknowledgement => PacketDirection::ServerBound,
            Self::LossStats => PacketDirection::ServerBound,
            Self::FrameStats => PacketDirection::ServerBound,
            Self::ControllerRumbleData => PacketDirection::ServerBound,
            Self::Termination => PacketDirection::ClientBound,
            Self::HdrMode => PacketDirection::ClientBound,
            Self::InputData => PacketDirection::ServerBound,
            Self::FrameFec => PacketDirection::ServerBound,
            Self::ControllerRumbleTriggers => PacketDirection::ServerBound,
            Self::ControllerSetMotion => PacketDirection::ClientBound,
            Self::ControllerSetLed => PacketDirection::ClientBound,
            Self::SetAdaptiveTriggers => PacketDirection::ClientBound,
        }
    }

    pub fn serialize(&self, config: &ControlPacketConfig) -> Option<RawControlPacketType> {
        match self {
            ControlPacketType::PeriodicPing => config.periodic_ping,
            ControlPacketType::RequestIdr => Some(config.request_idr),
            ControlPacketType::StartB => Some(config.start_b),
            ControlPacketType::InvalidateReferenceFrames => {
                Some(config.invalidate_reference_frames)
            }
            ControlPacketType::LongTermReferenceFrameAcknowledgement => {
                config.long_term_reference_frame_acknowledgement
            }
            ControlPacketType::LossStats => Some(config.loss_stats),
            ControlPacketType::FrameStats => Some(config.frame_stats),
            ControlPacketType::ControllerRumbleData => config.rumble_data,
            ControlPacketType::Termination => config.server_termination,
            ControlPacketType::HdrMode => config.hdr_mode,
            ControlPacketType::InputData => Some(config.input_data),
            ControlPacketType::FrameFec => config.frame_fec,
            ControlPacketType::ControllerRumbleTriggers => config.rumble_triggers,
            ControlPacketType::ControllerSetMotion => config.set_motion_event,
            ControlPacketType::ControllerSetLed => config.set_rgb_led,
            ControlPacketType::SetAdaptiveTriggers => config.set_adaptive_triggers,
        }
    }
    pub fn deserialize(
        direction: PacketDirection,
        config: &ControlPacketConfig,
        ty: RawControlPacketType,
    ) -> Option<Self> {
        match direction {
            PacketDirection::ClientBound => match ty {
                id if Some(id) == config.hdr_mode => Some(Self::HdrMode),
                id if Some(id) == config.server_termination => Some(Self::Termination),
                id if Some(id) == config.set_motion_event => Some(Self::ControllerSetMotion),
                id if Some(id) == config.set_rgb_led => Some(Self::ControllerSetLed),
                _ => None,
            },
            PacketDirection::ServerBound => match ty {
                id if Some(id) == config.periodic_ping => Some(Self::PeriodicPing),
                id if id == config.request_idr => Some(Self::RequestIdr),
                id if id == config.start_b => Some(Self::StartB),
                id if Some(id) == config.frame_fec => Some(Self::FrameFec),
                id if id == config.invalidate_reference_frames => {
                    Some(Self::InvalidateReferenceFrames)
                }
                id if Some(id) == config.long_term_reference_frame_acknowledgement => {
                    Some(Self::LongTermReferenceFrameAcknowledgement)
                }
                id if id == config.loss_stats => Some(Self::LossStats),
                id if id == config.frame_stats => Some(Self::FrameStats),
                id if id == config.input_data => Some(Self::InputData),
                _ => None,
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum TerminationReason {
    Long(u32),
    /// Prefer Long over Short
    Short(u16),
}

impl TerminationReason {
    /// Also known as NVST_DISCONN_SERVER_TERMINATED_CLOSED, ML_ERROR_GRACEFUL_TERMINATION or ML_ERROR_UNEXPECTED_EARLY_TERMINATION based on context and state.
    ///
    /// References:
    /// - Wolf: https://github.com/games-on-whales/wolf/blob/de3101881a7942dd67074d8ac0831febf50f6705/src/moonlight-protocol/moonlight/control.hpp#L300
    /// - Moonlight: https://github.com/moonlight-stream/moonlight-common-c/blob/62687809b1f7410c3db4be2527503a54ae408d70/src/ControlStream.c#L1320-L1330
    pub const GRACEFUL: TerminationReason = TerminationReason::Long(0x80030023);
    /// References:
    /// - Moonlight: https://github.com/moonlight-stream/moonlight-common-c/blob/62687809b1f7410c3db4be2527503a54ae408d70/src/ControlStream.c#L1314-L1316
    pub const NVST_DISCONN_SERVER_VIDEO_ENCODER_CONVERT_INPUT_FRAME_FAILED: TerminationReason =
        TerminationReason::Long(0x800e9403);
    /// References:
    /// - Moonlight: https://github.com/moonlight-stream/moonlight-common-c/blob/62687809b1f7410c3db4be2527503a54ae408d70/src/ControlStream.c#L1317-L1319
    pub const NVST_DISCONN_SERVER_VFP_PROTECTED_CONTENT: TerminationReason =
        TerminationReason::Long(0x800e9302);
}

/// References:
/// - https://github.com/moonlight-stream/moonlight-common-c/blob/7b026e77be62175104640e7e722b758df6d3d0d7/src/Input.h#L33
pub const UTF8_TEXT_MAX_COUNT: usize = 32;

/// References:
/// - https://github.com/moonlight-stream/moonlight-common-c/blob/7b026e77be62175104640e7e722b758df6d3d0d7/src/Input.h#L22
pub const KEY_DOWN_EVENT_MAGIC: u32 = 0x00000003;

/// References:
/// - https://github.com/moonlight-stream/moonlight-common-c/blob/7b026e77be62175104640e7e722b758df6d3d0d7/src/Input.h#L23
pub const KEY_UP_EVENT_MAGIC: u32 = 0x00000004;

/// This is for version ServerVersion::major < 4.
/// Those are not supported by this implementation
///
/// References:
/// - https://github.com/moonlight-stream/moonlight-common-c/blob/7b026e77be62175104640e7e722b758df6d3d0d7/src/Input.h#L39
pub const MOUSE_MOVE_REL_MAGIC: u32 = 0x00000006;

/// References:
/// - https://github.com/moonlight-stream/moonlight-common-c/blob/7b026e77be62175104640e7e722b758df6d3d0d7/src/Input.h#L39
pub const MOUSE_MOVE_REL_MAGIC_GEN5: u32 = 0x00000007;

/// References:
/// - https://github.com/moonlight-stream/moonlight-common-c/blob/7b026e77be62175104640e7e722b758df6d3d0d7/src/Input.h#L47
pub const MOUSE_MOVE_ABS_MAGIC: u32 = 0x00000005;

/// References:
/// - https://github.com/moonlight-stream/moonlight-common-c/blob/7b026e77be62175104640e7e722b758df6d3d0d7/src/Input.h#L62
pub const MOUSE_BUTTON_DOWN_EVENT_MAGIC_GEN5: u32 = 0x00000008;
/// References:
/// - https://github.com/moonlight-stream/moonlight-common-c/blob/7b026e77be62175104640e7e722b758df6d3d0d7/src/Input.h#L63
pub const MOUSE_BUTTON_UP_EVENT_MAGIC_GEN5: u32 = 0x00000009;

/// References:
/// - https://github.com/moonlight-stream/moonlight-common-c/blob/7b026e77be62175104640e7e722b758df6d3d0d7/src/Input.h#L69
pub const CONTROLLER_MAGIC: u32 = 0x0000000A;
/// References:
/// - https://github.com/moonlight-stream/moonlight-common-c/blob/7b026e77be62175104640e7e722b758df6d3d0d7/src/Input.h#L70
pub const C_HEADER_B: u32 = 0x1400;
/// References:
/// - https://github.com/moonlight-stream/moonlight-common-c/blob/7b026e77be62175104640e7e722b758df6d3d0d7/src/Input.h#L71
pub const C_TAIL_A: u32 = 0x0000009C;
/// References:
/// - https://github.com/moonlight-stream/moonlight-common-c/blob/7b026e77be62175104640e7e722b758df6d3d0d7/src/Input.h#L72
pub const C_TAIL_B: u32 = 0x0055;

/// This is for version ServerVersion::major < 4.
/// Those are not supported by this implementation
///
/// References:
/// - https://github.com/moonlight-stream/moonlight-common-c/blob/7b026e77be62175104640e7e722b758df6d3d0d7/src/Input.h#L87
pub const MULTI_CONTROLLER_MAGIC: u32 = 0x0000000D;
/// References:
/// - https://github.com/moonlight-stream/moonlight-common-c/blob/7b026e77be62175104640e7e722b758df6d3d0d7/src/Input.h#L88
pub const MULTI_CONTROLLER_MAGIC_GEN5: u32 = 0x0000000C;
/// References:
/// - https://github.com/moonlight-stream/moonlight-common-c/blob/7b026e77be62175104640e7e722b758df6d3d0d7/src/Input.h#L89
pub const MC_HEADER_B: i16 = 0x001A;
/// References:
/// - https://github.com/moonlight-stream/moonlight-common-c/blob/7b026e77be62175104640e7e722b758df6d3d0d7/src/Input.h#L72
pub const MC_MID_B: i16 = 0x0014;
/// References:
/// - https://github.com/moonlight-stream/moonlight-common-c/blob/7b026e77be62175104640e7e722b758df6d3d0d7/src/Input.h#L91
pub const MC_TAIL_A: i16 = 0x009C;
/// References:
/// - https://github.com/moonlight-stream/moonlight-common-c/blob/7b026e77be62175104640e7e722b758df6d3d0d7/src/Input.h#L92
pub const MC_TAIL_B: i16 = 0x0055;

/// This is for version ServerVersion::major < 4.
/// Those are not supported by this implementation
///
/// References:
/// - https://github.com/moonlight-stream/moonlight-common-c/blob/7b026e77be62175104640e7e722b758df6d3d0d7/src/Input.h#L111
pub const SCROLL_MAGIC: u32 = 0x00000009;
/// References:
/// - https://github.com/moonlight-stream/moonlight-common-c/blob/7b026e77be62175104640e7e722b758df6d3d0d7/src/Input.h#L112
pub const SCROLL_MAGIC_GEN5: u32 = 0x0000000A;
/// Matches Win32 WHEEL_DELTA definition
///
/// References:
/// - definition https://github.com/moonlight-stream/moonlight-common-c/blob/7b026e77be62175104640e7e722b758df6d3d0d7/src/InputStream.c#L36-L37
/// - usage: https://github.com/moonlight-stream/moonlight-common-c/blob/7b026e77be62175104640e7e722b758df6d3d0d7/src/InputStream.c#L1182-L1275
pub const SCROLL_DELTA: i16 = 120;

/// References:
/// - https://github.com/moonlight-stream/moonlight-common-c/blob/7b026e77be62175104640e7e722b758df6d3d0d7/src/Input.h#L120
pub const SS_HSCROLL_MAGIC: u32 = 0x55000001;

/// References:
/// - https://github.com/moonlight-stream/moonlight-common-c/blob/7b026e77be62175104640e7e722b758df6d3d0d7/src/Input.h#L126
pub const SS_TOUCH_MAGIC: u32 = 0x55000002;

/// References:
/// - https://github.com/moonlight-stream/moonlight-common-c/blob/7b026e77be62175104640e7e722b758df6d3d0d7/src/Input.h#L140
pub const SS_PEN_MAGIC: u32 = 0x55000003;

/// References:
/// - https://github.com/moonlight-stream/moonlight-common-c/blob/7b026e77be62175104640e7e722b758df6d3d0d7/src/Input.h#L157
pub const SS_CONTROLLER_ARRIVAL_MAGIC: u32 = 0x55000004;

/// References:
/// - https://github.com/moonlight-stream/moonlight-common-c/blob/7b026e77be62175104640e7e722b758df6d3d0d7/src/Input.h#L166
pub const SS_CONTROLLER_TOUCH_MAGIC: u32 = 0x55000005;

/// References:
/// - https://github.com/moonlight-stream/moonlight-common-c/blob/7b026e77be62175104640e7e722b758df6d3d0d7/src/Input.h#L178
pub const SS_CONTROLLER_MOTION_MAGIC: u32 = 0x55000006;

/// References:
/// - https://github.com/moonlight-stream/moonlight-common-c/blob/7b026e77be62175104640e7e722b758df6d3d0d7/src/Input.h#L189
pub const SS_CONTROLLER_BATTERY_MAGIC: u32 = 0x55000007;

/// References:
/// - https://github.com/moonlight-stream/moonlight-common-c/blob/7b026e77be62175104640e7e722b758df6d3d0d7/src/Input.h#L32
pub const UTF8_TEXT_EVENT_MAGIC: u32 = 0x00000017;

#[derive(Debug, PartialEq)]
pub enum ControlPacket {
    // -- Server Sent Events
    /// Sent from the server to set the controller rumble for a specific controller.
    ///
    /// References:
    /// - moonlight: https://github.com/moonlight-stream/moonlight-common-c/blob/7b026e77be62175104640e7e722b758df6d3d0d7/src/ControlStream.c#L1046-L1054
    /// - gow: https://games-on-whales.github.io/wolf/stable/protocols/control-specs.html#_rumble_data
    ControllerRumbleData {
        unused: u32,
        controller_number: u16,
        low_frequency: u16,
        high_frequency: u16,
    },
    /// Sunshine Extension
    ///
    /// Sent from the server to set the controller trigger rumble for a specific controller.
    ///
    /// References:
    /// - moonlight: https://github.com/moonlight-stream/moonlight-common-c/blob/7b026e77be62175104640e7e722b758df6d3d0d7/src/ControlStream.c#L1055-L1061
    /// - gow: https://games-on-whales.github.io/wolf/stable/protocols/control-specs.html#_rumble_triggers
    ControllerRumbleTriggers {
        controller_number: u16,
    },
    /// Sunshine Extension
    ///
    /// This is used to signal to Moonlight clients to start sending motion events (Gyro or Acceleration) to the server.
    /// By default Moonlight disables these events in order to save bandwith.
    ///
    /// References:
    /// - moonlight: https://github.com/moonlight-stream/moonlight-common-c/blob/7b026e77be62175104640e7e722b758df6d3d0d7/src/ControlStream.c#L1062-L1068
    /// - gow: https://games-on-whales.github.io/wolf/stable/protocols/control-specs.html#_motion_event
    ControllerSetMotion {
        controller_number: u16,
        rate: u16,
        motion_type: MotionType,
    },
    /// References:
    /// - moonlight: https://github.com/moonlight-stream/moonlight-common-c/blob/7b026e77be62175104640e7e722b758df6d3d0d7/src/ControlStream.c#L958-L981
    /// - gow: https://games-on-whales.github.io/wolf/stable/protocols/control-specs.html#_rgb_led
    ControllerSetLed {
        controller_number: u16,
        r: u8,
        g: u8,
        b: u8,
    },
    // -- Client Sent Events
    /// Also known as StartA
    RequestIdr,
    StartB,
    /// Must be sent every few milliseconds.
    /// Moonlight sends this every 100ms.
    /// APP_VERSION_AT_LEAST(7, 1, 415) is required.
    ///
    /// References:
    /// - Moonlight: https://github.com/moonlight-stream/moonlight-common-c/blob/2a5a1f3e8a57cbbb316ed7dfff3a3965c2e77d25/src/ControlStream.c#L1424-L1439
    /// - Moonlight Interval: https://github.com/moonlight-stream/moonlight-common-c/blob/2a5a1f3e8a57cbbb316ed7dfff3a3965c2e77d25/src/ControlStream.c#L298
    /// - Moonlight Version Check: https://github.com/moonlight-stream/moonlight-common-c/blob/2a5a1f3e8a57cbbb316ed7dfff3a3965c2e77d25/src/ControlStream.c#L354
    PeriodicPing,
    HdrMode {
        enabled: bool,
        /// Sunshine Extension
        ///
        /// References:
        /// - https://github.com/moonlight-stream/moonlight-common-c/blob/62687809b1f7410c3db4be2527503a54ae408d70/src/ControlStream.c#L1265-L1293
        sunshine: Option<SunshineHdrMetadata>,
    },
    /// Send the video loss statistics.
    /// This is used very regularly to update the server on the packet loss.
    ///
    /// References:
    /// - Moonlight: https://github.com/moonlight-stream/moonlight-common-c/blob/7b026e77be62175104640e7e722b758df6d3d0d7/src/ControlStream.c#L1451-L1484
    /// - Sunshine: https://github.com/LizardByte/Sunshine/blob/5fba591e6e856a3455d4c8a1cf2f62b68e699d02/src/stream.cpp#L935-L949
    LossStats {
        /// This is 0.
        unknown1: u32,
        /// The interval this packet gets sent in milliseconds.
        loss_report_interval_ms: u32,
        /// This is 1000
        unknown2: u32,
        /// The frame index from the depayloader of the last "good" frame.
        /// Good just means that we could fully receive it.
        last_good_frame: u64,
        /// This is 0.
        unknown3: u32,
        /// This is 0.
        unknown4: u32,
        /// This is 0x14.
        unknown5: u32,
    },
    /// Couldn't find any information on how to serialize / deserialize this packet.
    /// But it exists and is unused.
    ///
    /// References:
    /// - Moonlight unused: https://github.com/moonlight-stream/moonlight-common-c/blob/7b026e77be62175104640e7e722b758df6d3d0d7/src/ControlStream.c#L208
    /// - Sunshine unused: https://github.com/LizardByte/Sunshine/blob/ba4db46ac0bfbe478ad017f0b388bfcb346ad8ce/src/stream.cpp#L58
    FrameStats {},
    /// Sunshine Extension
    ///
    /// This doesn't actually seem to get used by Sunshine.
    /// Not sure why it exists then, but just use the [Self::LossStats] instead.
    ///
    /// Reports the video fec status to the server so it can adjust the amount of fec packets it sends.
    ///
    /// References:
    /// - moonlight sending: https://github.com/moonlight-stream/moonlight-common-c/blob/62687809b1f7410c3db4be2527503a54ae408d70/src/ControlStream.c#L1406-L1421
    /// - moonlight definition: https://github.com/moonlight-stream/moonlight-common-c/blob/62687809b1f7410c3db4be2527503a54ae408d70/src/Video.h#L56-L70
    /// - Sunshine not used: https://github.com/LizardByte/Sunshine/blob/5fba591e6e856a3455d4c8a1cf2f62b68e699d02/src/stream.cpp#L922-L1174
    FrameFec {
        frame_index: u32,
        highest_received_sequence_number: u16,
        next_contiguous_sequence_number: u16,
        missing_packets_before_highest_received: u16,
        total_data_packets: u16,
        total_parity_packets: u16,
        received_data_packets: u16,
        received_parity_packets: u16,
        fec_percentage: u8,
        multi_fec_block_index: u8,
        multi_fec_block_count: u8,
    },
    // --- Inputs ---
    /// Moves the mouse using relative motion
    ///
    /// References:
    /// - https://games-on-whales.github.io/wolf/stable/protocols/input-data.html#_mouse_relative_move
    /// - https://github.com/games-on-whales/wolf/blob/5a393daafac36ff86453504d96faea50d160780d/src/moonlight-protocol/moonlight/control.hpp#L130-L133
    /// - https://github.com/moonlight-stream/moonlight-common-c/blob/7b026e77be62175104640e7e722b758df6d3d0d7/src/Input.h#L41-L45
    MouseMoveRelative {
        delta_x: i16,
        delta_y: i16,
    },
    /// Moves the mouse to x and y based on the reference width and height
    ///
    /// References:
    /// - https://github.com/games-on-whales/wolf/blob/5a393daafac36ff86453504d96faea50d160780d/src/moonlight-protocol/moonlight/control.hpp#L135-L141
    /// - https://github.com/moonlight-stream/moonlight-common-c/blob/7b026e77be62175104640e7e722b758df6d3d0d7/src/Input.h#L48-L60
    MouseMoveAbsolute {
        x: i16,
        y: i16,
        /// This is 0.
        unused: i16,
        reference_width: i16,
        reference_height: i16,
    },
    /// References:
    /// - https://games-on-whales.github.io/wolf/stable/protocols/input-data.html#_mouse_button
    /// - https://github.com/games-on-whales/wolf/blob/5a393daafac36ff86453504d96faea50d160780d/src/moonlight-protocol/moonlight/control.hpp#L143-L145
    /// - https://github.com/moonlight-stream/moonlight-common-c/blob/7b026e77be62175104640e7e722b758df6d3d0d7/src/Input.h#L64-L67
    MouseButton {
        action: MouseButtonAction,
        button: MouseButton,
    },
    /// Sends a keyboard event to the host.
    ///
    /// Weird side effects:
    /// - https://github.com/moonlight-stream/moonlight-common-c/blob/62687809b1f7410c3db4be2527503a54ae408d70/src/InputStream.c#L902-L943
    ///
    /// References:
    /// - https://games-on-whales.github.io/wolf/stable/protocols/input-data.html#_keyboard
    /// - https://github.com/games-on-whales/wolf/blob/5a393daafac36ff86453504d96faea50d160780d/src/moonlight-protocol/moonlight/control.hpp#L157-L162
    Keyboard {
        action: KeyAction,
        flags: KeyFlags,
        key_code: KeyCode,
        modifiers: KeyModifiers,
        zero: i16,
    },
    /// Sends utf8 encoded text to the host.
    ///
    /// References:
    /// - Moonlight layout: https://github.com/moonlight-stream/moonlight-common-c/blob/7b026e77be62175104640e7e722b758df6d3d0d7/src/Input.h#L32-L37
    /// - Moonlight construction: https://github.com/moonlight-stream/moonlight-common-c/blob/62687809b1f7410c3db4be2527503a54ae408d70/src/InputStream.c#L983-L985
    Text {
        text: [u8; UTF8_TEXT_MAX_COUNT],
        /// Must be smaller or equal to 32
        text_len: usize,
    },
    /// Vertical Scrolling.
    ///
    /// Only use scroll_amount_1.
    ///
    /// References:
    /// - https://games-on-whales.github.io/wolf/stable/protocols/input-data.html#_mouse_scroll
    /// - https://github.com/games-on-whales/wolf/blob/5a393daafac36ff86453504d96faea50d160780d/src/moonlight-protocol/moonlight/control.hpp#L147-L151
    /// - https://github.com/moonlight-stream/moonlight-common-c/blob/7b026e77be62175104640e7e722b758df6d3d0d7/src/Input.h#L113-L118
    MouseScroll {
        scroll_amount_1: i16,
        /// This is unused
        scroll_amount_2: i16,
        /// This should be zero
        zero: i16,
    },
    /// Horizontal Scrolling
    ///
    /// References:
    /// - https://games-on-whales.github.io/wolf/stable/protocols/input-data.html#_mouse_horizontal_scroll
    /// - https://github.com/moonlight-stream/moonlight-common-c/blob/7b026e77be62175104640e7e722b758df6d3d0d7/src/Input.h#L121-L124
    MouseHorizontalScroll {
        scroll_amount: i16,
    },
    /// Sunshine Extension
    ///
    /// Send touch event to the host.
    ///
    /// These are detect using the sdp
    ///
    /// See also:
    /// - https://github.com/moonlight-stream/moonlight-common-c/blob/7b026e77be62175104640e7e722b758df6d3d0d7/src/Limelight.h#L615-L650
    ///
    /// References:
    /// - https://github.com/moonlight-stream/moonlight-common-c/blob/7b026e77be62175104640e7e722b758df6d3d0d7/src/Input.h#L127-L138
    /// - sdp detection: https://github.com/moonlight-stream/moonlight-common-c/blob/7b026e77be62175104640e7e722b758df6d3d0d7/src/RtspConnection.c#L1145-L1147
    Touch {
        event_type: TouchEventType,
        /// This is 0.
        reserved: u8,
        rotation: u16,
        pointer_id: u32,
        x: f32,
        y: f32,
        pressure_or_distance: f32,
        contact_area_minor: f32,
        contact_area_major: f32,
    },
    /// Sunshine Extension
    ///
    /// Sends pen related events to the host.
    ///
    /// References:
    /// - definition: https://github.com/moonlight-stream/moonlight-common-c/blob/7b026e77be62175104640e7e722b758df6d3d0d7/src/Input.h#L141-L155
    /// - other types related: https://github.com/moonlight-stream/moonlight-common-c/blob/7b026e77be62175104640e7e722b758df6d3d0d7/src/Limelight.h#L663-L682
    Pen {
        event_type: TouchEventType,
        tool_type: ToolType,
        buttons: PenButtons,
        /// This is zero.
        zero: u8,
        x: f32,
        y: f32,
        pressure_or_distance: f32,
        rotation: u16,
        tilt: u8,
        /// This is zero.
        zero2: u8,
        contact_area_minor: f32,
        contact_area_major: f32,
    },
    /// Send controller inputs to the host.
    ///
    /// This is the MultiController packet in moonlight, but we only have this because we only support Gen5 server and only server below Gen5 use the old packet.
    ///
    /// References:
    /// - https://github.com/moonlight-stream/moonlight-common-c/blob/7b026e77be62175104640e7e722b758df6d3d0d7/src/Input.h#L93-L109
    /// - how to use correctly: https://github.com/moonlight-stream/moonlight-common-c/blob/7b026e77be62175104640e7e722b758df6d3d0d7/src/InputStream.c#L998-L1162
    ControllerState {
        header_b: i16,
        controller_number: i16,
        active_gamepad_mask: ActiveGamepads,
        /// This is [MC_MID_B]
        mid_b: i16,
        /// This is using the first two bytes of [ControllerButtons]
        button_flags: i16,
        left_trigger: u8,
        right_trigger: u8,
        left_stick_x: i16,
        left_stick_y: i16,
        right_stick_x: i16,
        right_stick_y: i16,
        /// This is [MC_TAIL_A]
        tail_a: i16,
        /// Sunshine Extension
        /// This is using the last two bytes of [ControllerButtons]
        ///
        /// For GFE always 0.
        ///
        /// References:
        /// - https://github.com/moonlight-stream/moonlight-common-c/blob/7b026e77be62175104640e7e722b758df6d3d0d7/src/Input.h#L107
        button_flags_2: i16,
        /// This is [MC_TAIL_B]
        tail_b: i16,
    },
    /// Sunshine Extension
    ///
    /// Send the arrival of a controller to the host.
    ///
    /// References:
    /// - https://github.com/moonlight-stream/moonlight-common-c/blob/7b026e77be62175104640e7e722b758df6d3d0d7/src/Input.h#L158-L164
    ControllerArrival {
        controller_number: u8,
        ty: ControllerType,
        capabilities: ControllerCapabilities,
        supported_buttons: ControllerButtons,
    },
    /// Sunshine Extension
    ///
    /// Send controller motion to the host.
    ///
    /// References:
    /// - https://github.com/moonlight-stream/moonlight-common-c/blob/7b026e77be62175104640e7e722b758df6d3d0d7/src/Input.h#L179-L187
    ControllerMotion {
        controller_number: u8,
        motion_type: MotionType,
        reserved: [u8; 2],
        x: f32,
        y: f32,
        z: f32,
    },
    /// References:
    /// - https://github.com/moonlight-stream/moonlight-common-c/blob/7b026e77be62175104640e7e722b758df6d3d0d7/src/Input.h#L190-L196
    /// - how to use: https://github.com/moonlight-stream/moonlight-common-c/blob/7b026e77be62175104640e7e722b758df6d3d0d7/src/InputStream.c#L1588-L1628
    ControllerBattery {
        // TODO: what does this do exactly?
        controller_number: u8,
        battery_state: BatteryState,
        battery_percentage: u8,
        /// This is 0.
        reserved: u8,
    },
    /// Invalidates references frames. Make sure the server supports this using the sdp before requesting this.
    ///
    /// References:
    /// - https://github.com/moonlight-stream/moonlight-common-c/blob/7b026e77be62175104640e7e722b758df6d3d0d7/src/Video.h#L79-L86
    InvalidateReferenceFrames {
        first_frame_index: u32,
        reserved1: u32,
        last_frame_index: u32,
        reserved2: [u32; 3],
    },
    /// Acknowledges a long term reference frame.
    ///
    /// References:
    /// - https://github.com/moonlight-stream/moonlight-common-c/blob/7b026e77be62175104640e7e722b758df6d3d0d7/src/Video.h#L72-L77
    LongTermReferenceFrameAcknowledgement {
        frame_index: u32,
        reserved: u32,
    },
    /// The server terminated the session.
    /// There also seems to be a short termination code, however this is not implemented.
    ///
    /// Client Termination works by disconnecting using enet.
    ///
    /// References:
    /// - Wolf: https://github.com/games-on-whales/wolf/blob/de3101881a7942dd67074d8ac0831febf50f6705/src/moonlight-protocol/moonlight/control.hpp#L300-L310
    /// - Moonlight: https://github.com/moonlight-stream/moonlight-common-c/blob/62687809b1f7410c3db4be2527503a54ae408d70/src/ControlStream.c#L1299-L1379
    /// - Moonlight short: https://github.com/moonlight-stream/moonlight-common-c/blob/62687809b1f7410c3db4be2527503a54ae408d70/src/ControlStream.c#L1336-L1355
    /// - Moonlight Client Disconnect: https://github.com/moonlight-stream/moonlight-common-c/blob/7b026e77be62175104640e7e722b758df6d3d0d7/src/Misc.c#L43-L85
    ServerTermination {
        reason: TerminationReason,
    },
}

impl ControlPacket {
    /// A simple wrapper for [Self::Text]
    pub fn text(text: &str) -> Option<Self> {
        let mut text_array = [0; _];
        if text.len() > text_array.len() {
            return None;
        }
        text_array[0..text.len()].copy_from_slice(text.as_bytes());

        Some(Self::Text {
            text: text_array,
            text_len: text.len(),
        })
    }

    /// A simple wrapper for [Self::ControllerState]
    ///
    /// left_trigger, right_trigger are values between or equal 0..1
    /// left_stick_x, left_stick_y, right_stick_x, right_stick_y are values between or equal -1..1
    pub fn controller_state(
        mask: ActiveGamepads,
        controller_number: i16,
        button_flags: ControllerButtons,
        left_trigger: f32,
        right_trigger: f32,
        left_stick_x: f32,
        left_stick_y: f32,
        right_stick_x: f32,
        right_stick_y: f32,
    ) -> Self {
        // See https://github.com/moonlight-stream/moonlight-common-c/blob/7b026e77be62175104640e7e722b758df6d3d0d7/src/InputStream.c#L1104-L1128

        Self::ControllerState {
            header_b: MC_HEADER_B,
            controller_number,
            active_gamepad_mask: mask,
            mid_b: MC_MID_B,
            button_flags: (button_flags.bits() & 0x0000_FFFF) as i16,
            left_trigger: (left_trigger.clamp(0.0, 1.0) * u8::MAX as f32) as u8,
            right_trigger: (right_trigger.clamp(0.0, 1.0) * u8::MAX as f32) as u8,
            left_stick_x: (left_stick_x.clamp(-1.0, 1.0) * i16::MAX as f32) as i16,
            left_stick_y: (left_stick_y.clamp(-1.0, 1.0) * i16::MAX as f32) as i16,
            right_stick_x: (right_stick_x.clamp(-1.0, 1.0) * i16::MAX as f32) as i16,
            right_stick_y: (right_stick_y.clamp(-1.0, 1.0) * i16::MAX as f32) as i16,
            tail_a: MC_TAIL_A,
            button_flags_2: ((button_flags.bits() >> 16) & 0x0000_FFFF) as i16,
            tail_b: MC_TAIL_B,
        }
    }

    pub fn channel(&self, server_version: ServerVersion) -> (EnetChannel, PacketKind) {
        // All packets from host to client are reliable and on channel 0
        // See https://github.com/LizardByte/Sunshine/blob/5364b008c0ada0ab90d27bd991f21951fafffad7/src/stream.cpp#L299-L308
        if server_version.is_sunshine_like() {
            match self {
                // request idr: https://github.com/moonlight-stream/moonlight-common-c/blob/62687809b1f7410c3db4be2527503a54ae408d70/src/ControlStream.c#L1522-L1528
                // ltr ack: https://github.com/moonlight-stream/moonlight-common-c/blob/62687809b1f7410c3db4be2527503a54ae408d70/src/ControlStream.c#L1569-L1575
                // invalidate ref frames: https://github.com/moonlight-stream/moonlight-common-c/blob/62687809b1f7410c3db4be2527503a54ae408d70/src/ControlStream.c#L1509-L1515
                ControlPacket::RequestIdr
                | ControlPacket::StartB
                | ControlPacket::LongTermReferenceFrameAcknowledgement { .. } => {
                    (EnetChannel::CHANNEL_URGENT, PacketKind::Reliable)
                }
                // loss stats: https://github.com/moonlight-stream/moonlight-common-c/blob/62687809b1f7410c3db4be2527503a54ae408d70/src/ControlStream.c#L1469-L1475
                // frame fec: https://github.com/moonlight-stream/moonlight-common-c/blob/62687809b1f7410c3db4be2527503a54ae408d70/src/ControlStream.c#L1407-L1413
                ControlPacket::LossStats { .. } | ControlPacket::FrameFec { .. } => (
                    EnetChannel::CHANNEL_GENERIC,
                    PacketKind::Unreliable { sequenced: false },
                ),
                // See: https://github.com/moonlight-stream/moonlight-common-c/blob/2a5a1f3e8a57cbbb316ed7dfff3a3965c2e77d25/src/ControlStream.c#L1424-L1429
                // Send the message (and don't expect a response)
                //
                // NB: We send this periodic message as reliable to ensure the RTT is recomputed
                // regularly. This only happens when an ACK is received to a reliable packet.
                // Since the other traffic on this channel is unsequenced, it doesn't really
                // cause any negative HOL blocking side-effects.
                ControlPacket::PeriodicPing => (EnetChannel::CHANNEL_GENERIC, PacketKind::Reliable),
                // https://github.com/moonlight-stream/moonlight-common-c/blob/62687809b1f7410c3db4be2527503a54ae408d70/src/InputStream.c#L738-L742
                ControlPacket::MouseMoveRelative { .. } => {
                    (EnetChannel::CHANNEL_MOUSE, PacketKind::Reliable)
                }
                // https://github.com/moonlight-stream/moonlight-common-c/blob/62687809b1f7410c3db4be2527503a54ae408d70/src/InputStream.c#L803-L806
                ControlPacket::MouseMoveAbsolute { .. } => {
                    (EnetChannel::CHANNEL_MOUSE, PacketKind::Reliable)
                }
                // https://github.com/moonlight-stream/moonlight-common-c/blob/62687809b1f7410c3db4be2527503a54ae408d70/src/InputStream.c#L865-L866
                ControlPacket::MouseButton { .. } => {
                    (EnetChannel::CHANNEL_MOUSE, PacketKind::Reliable)
                }
                // https://github.com/moonlight-stream/moonlight-common-c/blob/62687809b1f7410c3db4be2527503a54ae408d70/src/InputStream.c#L899-L900
                ControlPacket::Keyboard { .. } => {
                    (EnetChannel::CHANNEL_KEYBOARD, PacketKind::Reliable)
                }
                // https://github.com/moonlight-stream/moonlight-common-c/blob/62687809b1f7410c3db4be2527503a54ae408d70/src/InputStream.c#L980-L981
                ControlPacket::Text { .. } => (EnetChannel::CHANNEL_UTF8, PacketKind::Reliable),
                // https://github.com/moonlight-stream/moonlight-common-c/blob/7b026e77be62175104640e7e722b758df6d3d0d7/src/InputStream.c#L1445-L1447
                ControlPacket::ControllerArrival {
                    controller_number, ..
                } => (
                    EnetChannel::controller(*controller_number)
                        .unwrap_or(EnetChannel::CHANNEL_GAMEPAD_BASE),
                    PacketKind::Reliable,
                ),
                // https://github.com/moonlight-stream/moonlight-common-c/blob/7b026e77be62175104640e7e722b758df6d3d0d7/src/InputStream.c#L1609-L1611
                ControlPacket::ControllerBattery {
                    controller_number, ..
                } => (
                    EnetChannel::controller(*controller_number)
                        .unwrap_or(EnetChannel::CHANNEL_GAMEPAD_BASE),
                    PacketKind::Reliable,
                ),
                // channel: https://github.com/moonlight-stream/moonlight-common-c/blob/7b026e77be62175104640e7e722b758df6d3d0d7/src/InputStream.c#L1558-L1559
                // reliable or not: https://github.com/moonlight-stream/moonlight-common-c/blob/7b026e77be62175104640e7e722b758df6d3d0d7/src/InputStream.c#L525-L534
                // Motion events are so rapid that we can just drop any events that are lost in transit,
                // but we will treat (0, 0, 0) as a special value for gyro events to allow clients to
                // reliably set the gyro to a null state when sensor events are halted due to focus loss
                // or similar client-side constraints.
                ControlPacket::ControllerMotion {
                    controller_number,
                    motion_type,
                    x,
                    y,
                    z,
                    ..
                } => (
                    EnetChannel::controller(*controller_number)
                        .unwrap_or(EnetChannel::CHANNEL_GAMEPAD_BASE),
                    if *motion_type == MotionType::Gyroscope && *x == 0.0 && *y == 0.0 && *z == 0.0
                    {
                        PacketKind::Reliable
                    } else {
                        // moonlight-common-c doesn't set the sequenced flag, however it does make sense to just set it to drop older packets
                        PacketKind::Unreliable { sequenced: true }
                    },
                ),
                ControlPacket::ControllerRumbleData {
                    controller_number, ..
                } => (
                    // Server Packet, see above
                    EnetChannel::controller(*controller_number as u8)
                        .unwrap_or(EnetChannel::CHANNEL_GAMEPAD_BASE),
                    PacketKind::Reliable,
                ),
                ControlPacket::ControllerRumbleTriggers {
                    controller_number, ..
                } => (
                    // Server Packet, see above
                    EnetChannel::controller(*controller_number as u8)
                        .unwrap_or(EnetChannel::CHANNEL_GAMEPAD_BASE),
                    PacketKind::Reliable,
                ),
                //
                ControlPacket::ControllerSetLed {
                    controller_number, ..
                } => (
                    // Server Packet, see above
                    EnetChannel::controller(*controller_number as u8)
                        .unwrap_or(EnetChannel::CHANNEL_GAMEPAD_BASE),
                    PacketKind::Reliable,
                ),
                ControlPacket::ControllerSetMotion {
                    controller_number, ..
                } => (
                    // Server Packet, see above
                    EnetChannel::controller(*controller_number as u8)
                        .unwrap_or(EnetChannel::CHANNEL_GAMEPAD_BASE),
                    PacketKind::Reliable,
                ),
                ControlPacket::ControllerState {
                    controller_number, ..
                } => (
                    // moonlight has todo to send them Unreliable and sequenced, so we do that
                    // https://github.com/moonlight-stream/moonlight-common-c/blob/7b026e77be62175104640e7e722b758df6d3d0d7/src/InputStream.c#L1072-L1076
                    EnetChannel::controller(*controller_number as u8)
                        .unwrap_or(EnetChannel::CHANNEL_GAMEPAD_BASE),
                    PacketKind::Unreliable { sequenced: true },
                ),
                _ => todo!("{:?}", self),
            }
        } else {
            // https://github.com/moonlight-stream/moonlight-common-c/blob/2a5a1f3e8a57cbbb316ed7dfff3a3965c2e77d25/src/ControlStream.c#L763-L767
            // Always use channel 0 and reliable for GFE
            (EnetChannel::CHANNEL_GENERIC, PacketKind::Reliable)
        }
    }

    /// This is the maximum size a packet can have
    pub const MAX_SIZE: usize = 44;

    pub fn ty(&self) -> ControlPacketType {
        match self {
            Self::PeriodicPing => ControlPacketType::PeriodicPing,
            Self::RequestIdr => ControlPacketType::RequestIdr,
            Self::StartB => ControlPacketType::StartB,
            Self::InvalidateReferenceFrames { .. } => ControlPacketType::InvalidateReferenceFrames,
            Self::LongTermReferenceFrameAcknowledgement { .. } => {
                ControlPacketType::LongTermReferenceFrameAcknowledgement
            }
            Self::LossStats { .. } => ControlPacketType::LossStats,
            Self::FrameStats { .. } => ControlPacketType::FrameStats,
            Self::FrameFec { .. } => ControlPacketType::FrameFec,
            Self::ControllerRumbleData { .. } => ControlPacketType::ControllerRumbleData,
            Self::ControllerRumbleTriggers { .. } => ControlPacketType::ControllerRumbleTriggers,
            Self::ControllerSetMotion { .. } => ControlPacketType::ControllerSetMotion,
            Self::ControllerSetLed { .. } => ControlPacketType::ControllerSetLed,
            Self::ServerTermination { .. } => ControlPacketType::Termination,
            Self::HdrMode { .. } => ControlPacketType::HdrMode,
            Self::MouseButton { .. } => ControlPacketType::InputData,
            Self::MouseMoveRelative { .. } => ControlPacketType::InputData,
            Self::MouseMoveAbsolute { .. } => ControlPacketType::InputData,
            Self::MouseScroll { .. } => ControlPacketType::InputData,
            Self::MouseHorizontalScroll { .. } => ControlPacketType::InputData,
            Self::Keyboard { .. } => ControlPacketType::InputData,
            Self::Text { .. } => ControlPacketType::InputData,
            Self::Touch { .. } => ControlPacketType::InputData,
            Self::Pen { .. } => ControlPacketType::InputData,
            Self::ControllerState { .. } => ControlPacketType::InputData,
            Self::ControllerArrival { .. } => ControlPacketType::InputData,
            Self::ControllerMotion { .. } => ControlPacketType::InputData,
            Self::ControllerBattery { .. } => ControlPacketType::InputData,
        }
    }

    /// Buffer is:
    /// - If not encrypted: the full payload
    /// - If encrypted: the decrypted payload -> it needs to be encrypted
    #[instrument(level = Level::TRACE, skip(config))]
    pub fn serialize(
        &self,
        config: &ControlPacketConfig,
        buffer: &mut [u8; Self::MAX_SIZE],
    ) -> Result<usize, ControlPacketNotSupported> {
        match self {
            Self::PeriodicPing => {
                let ty = config.periodic_ping.ok_or(ControlPacketNotSupported)?;

                buffer[0..2].copy_from_slice(&ty.to_le_bytes());

                // Length of payload
                buffer[2..4].copy_from_slice(&4u16.to_le_bytes());

                // Timestamp?
                buffer[4..8].copy_from_slice(&[0, 0, 0, 0]);

                Ok(8)
            }
            Self::HdrMode { enabled, sunshine } => {
                // Ty
                let ty = config.hdr_mode.ok_or(ControlPacketNotSupported)?;
                buffer[0..2].copy_from_slice(&ty.to_le_bytes());

                // Length later

                // Data
                buffer[4] = *enabled as u8;

                let payload_len = if let Some(metadata) = sunshine {
                    let mut serialize_primary = |i: usize, primary: Primary| {
                        buffer[i..(i + 2)].copy_from_slice(&primary.x.to_le_bytes());
                        buffer[(i + 2)..(i + 4)].copy_from_slice(&primary.y.to_le_bytes());
                    };

                    serialize_primary(5, metadata.display_primaries[0]);
                    serialize_primary(9, metadata.display_primaries[1]);
                    serialize_primary(13, metadata.display_primaries[2]);
                    serialize_primary(17, metadata.white_point);

                    buffer[21..23].copy_from_slice(&metadata.max_display_luminance.to_le_bytes());
                    buffer[23..25].copy_from_slice(&metadata.min_display_luminance.to_le_bytes());

                    buffer[25..27].copy_from_slice(&metadata.max_content_light_level.to_le_bytes());

                    buffer[27..29]
                        .copy_from_slice(&metadata.max_frame_average_light_level.to_le_bytes());

                    buffer[29..31]
                        .copy_from_slice(&metadata.max_full_frame_luminance.to_le_bytes());

                    27
                } else {
                    1
                };

                // Length
                buffer[2..4].copy_from_slice(&(payload_len as u16).to_le_bytes());

                // 4 = type + packet length
                Ok(4 + payload_len)
            }
            Self::RequestIdr => {
                // Ty
                let ty = config.request_idr;
                buffer[0..2].copy_from_slice(&ty.to_le_bytes());

                // Length later

                // https://github.com/moonlight-stream/moonlight-common-c/blob/435bc6a5a4852c90cfb037de1378c0334ed36d8e/src/ControlStream.c#L218-L227
                let contents = [0, 0];

                buffer[4..(contents.len() + 4)].copy_from_slice(&contents);

                // Length
                buffer[2..4].copy_from_slice(&(contents.len() as u16).to_le_bytes());

                Ok(4 + contents.len())
            }
            Self::StartB => {
                // Ty
                let ty = config.start_b;
                buffer[0..2].copy_from_slice(&ty.to_le_bytes());

                // Length later

                // https://github.com/moonlight-stream/moonlight-common-c/blob/435bc6a5a4852c90cfb037de1378c0334ed36d8e/src/ControlStream.c#L218-L227
                let contents: &[u8] = match config.server_version.major {
                    3 => &[0, 0, 0, 0xa],
                    _ => &[0],
                };

                buffer[4..(contents.len() + 4)].copy_from_slice(contents);

                // Length
                buffer[2..4].copy_from_slice(&(contents.len() as u16).to_le_bytes());

                Ok(4 + contents.len())
            }
            Self::LossStats {
                unknown1,
                loss_report_interval_ms,
                unknown2,
                last_good_frame,
                unknown3,
                unknown4,
                unknown5,
            } => {
                // Ty
                let ty = config.loss_stats;
                buffer[0..2].copy_from_slice(&ty.to_le_bytes());

                // Length
                let content_len: u16 = 32;
                buffer[2..4].copy_from_slice(&content_len.to_le_bytes());

                // Data
                buffer[4..8].copy_from_slice(&unknown1.to_le_bytes());
                buffer[8..12].copy_from_slice(&loss_report_interval_ms.to_le_bytes());
                buffer[12..16].copy_from_slice(&unknown2.to_le_bytes());
                buffer[16..24].copy_from_slice(&last_good_frame.to_le_bytes());
                buffer[24..28].copy_from_slice(&unknown3.to_le_bytes());
                buffer[28..32].copy_from_slice(&unknown4.to_le_bytes());
                buffer[32..36].copy_from_slice(&unknown5.to_le_bytes());

                Ok(4 + content_len as usize)
            }
            Self::FrameStats {} => {
                // Ty
                let ty = config.frame_stats;
                buffer[0..2].copy_from_slice(&ty.to_le_bytes());

                // Length
                let content_len: u16 = 0;
                buffer[2..4].copy_from_slice(&content_len.to_le_bytes());

                // Data, unknown
                warn!("FrameStats packet payload is unknown by this implementation");

                Ok(4 + content_len as usize)
            }
            Self::FrameFec {
                frame_index,
                highest_received_sequence_number,
                next_contiguous_sequence_number,
                missing_packets_before_highest_received,
                total_data_packets,
                total_parity_packets,
                received_data_packets,
                received_parity_packets,
                fec_percentage,
                multi_fec_block_index,
                multi_fec_block_count,
            } => {
                // Ty
                let ty = config.frame_fec.ok_or(ControlPacketNotSupported)?;
                buffer[0..2].copy_from_slice(&ty.to_le_bytes());

                // Length
                let content_len: u16 = 21;
                buffer[2..4].copy_from_slice(&content_len.to_le_bytes());

                // Data
                buffer[4..8].copy_from_slice(&frame_index.to_be_bytes());
                buffer[8..10].copy_from_slice(&highest_received_sequence_number.to_be_bytes());
                buffer[10..12].copy_from_slice(&next_contiguous_sequence_number.to_be_bytes());
                buffer[12..14]
                    .copy_from_slice(&missing_packets_before_highest_received.to_be_bytes());
                buffer[14..16].copy_from_slice(&total_data_packets.to_be_bytes());
                buffer[16..18].copy_from_slice(&total_parity_packets.to_be_bytes());
                buffer[18..20].copy_from_slice(&received_data_packets.to_be_bytes());
                buffer[20..22].copy_from_slice(&received_parity_packets.to_be_bytes());
                buffer[22..23].copy_from_slice(&fec_percentage.to_be_bytes());
                buffer[23..24].copy_from_slice(&multi_fec_block_index.to_be_bytes());
                buffer[24..25].copy_from_slice(&multi_fec_block_count.to_be_bytes());

                Ok(4 + content_len as usize)
            }
            Self::InvalidateReferenceFrames {
                first_frame_index,
                reserved1,
                last_frame_index,
                reserved2,
            } => {
                let ty = config.invalidate_reference_frames;
                buffer[0..2].copy_from_slice(&ty.to_le_bytes());

                // payload size = 4 * 6 = 24 bytes
                let content_len: u16 = 24;
                buffer[2..4].copy_from_slice(&content_len.to_le_bytes());

                buffer[4..8].copy_from_slice(&first_frame_index.to_le_bytes());
                buffer[8..12].copy_from_slice(&reserved1.to_le_bytes());
                buffer[12..16].copy_from_slice(&last_frame_index.to_le_bytes());

                for i in 0..3 {
                    let start = 16 + i * 4;
                    buffer[start..start + 4].copy_from_slice(&reserved2[i].to_le_bytes());
                }

                Ok(4 + content_len as usize)
            }
            Self::LongTermReferenceFrameAcknowledgement {
                frame_index,
                reserved,
            } => {
                let ty = config
                    .long_term_reference_frame_acknowledgement
                    .ok_or(ControlPacketNotSupported)?;

                buffer[0..2].copy_from_slice(&ty.to_le_bytes());

                // payload = 8 bytes
                let content_len: u16 = 8;
                buffer[2..4].copy_from_slice(&content_len.to_le_bytes());

                buffer[4..8].copy_from_slice(&frame_index.to_le_bytes());
                buffer[8..12].copy_from_slice(&reserved.to_le_bytes());

                Ok(4 + content_len as usize)
            }
            Self::MouseMoveRelative { delta_x, delta_y } => {
                // Ty
                let ty = config.input_data;
                buffer[0..2].copy_from_slice(&ty.to_le_bytes());

                // Length
                let input_len: u32 = 8;
                let content_len: u16 = 4 + input_len as u16;
                buffer[2..4].copy_from_slice(&content_len.to_le_bytes());

                // Input Len
                buffer[4..8].copy_from_slice(&input_len.to_be_bytes());

                // Input Ty
                let ty: u32 = if config.server_version.major >= 5 {
                    MOUSE_MOVE_REL_MAGIC_GEN5
                } else {
                    MOUSE_MOVE_REL_MAGIC
                };
                buffer[8..12].copy_from_slice(&ty.to_le_bytes());

                // Data
                buffer[12..14].copy_from_slice(&delta_x.to_be_bytes());
                buffer[14..16].copy_from_slice(&delta_y.to_be_bytes());

                Ok(4 + content_len as usize)
            }
            Self::MouseMoveAbsolute {
                x,
                y,
                unused,
                reference_width,
                reference_height,
            } => {
                // Ty
                let ty = config.input_data;
                buffer[0..2].copy_from_slice(&ty.to_le_bytes());

                // Length
                let input_len: u32 = 14;
                let content_len: u16 = 4 + input_len as u16;
                buffer[2..4].copy_from_slice(&content_len.to_le_bytes());

                // Input Len
                buffer[4..8].copy_from_slice(&input_len.to_be_bytes());

                // Input Ty
                let ty: u32 = MOUSE_MOVE_ABS_MAGIC;
                buffer[8..12].copy_from_slice(&ty.to_le_bytes());

                // Data
                buffer[12..14].copy_from_slice(&x.to_be_bytes());
                buffer[14..16].copy_from_slice(&y.to_be_bytes());
                buffer[16..18].copy_from_slice(&unused.to_be_bytes());

                buffer[18..20].copy_from_slice(&reference_width.to_be_bytes());
                buffer[20..22].copy_from_slice(&reference_height.to_be_bytes());

                Ok(4 + content_len as usize)
            }
            Self::MouseButton { action, button } => {
                // Ty
                let ty = config.input_data;
                buffer[0..2].copy_from_slice(&ty.to_le_bytes());

                // Length
                let input_len: u32 = 5;
                let content_len: u16 = 4 + input_len as u16;
                buffer[2..4].copy_from_slice(&content_len.to_le_bytes());

                // Input Len
                buffer[4..8].copy_from_slice(&input_len.to_be_bytes());

                // Input Ty
                let ty: u32 = match action {
                    MouseButtonAction::Press => MOUSE_BUTTON_DOWN_EVENT_MAGIC_GEN5,
                    MouseButtonAction::Release => MOUSE_BUTTON_UP_EVENT_MAGIC_GEN5,
                };
                buffer[8..12].copy_from_slice(&ty.to_le_bytes());

                // Data
                buffer[12..13].copy_from_slice(&[*button as u8]);

                Ok(4 + content_len as usize)
            }
            Self::MouseScroll {
                scroll_amount_1,
                scroll_amount_2,
                zero,
            } => {
                // https://games-on-whales.github.io/wolf/stable/protocols/input-data.html#_mouse_scroll
                // Ty
                let ty = config.input_data;
                buffer[0..2].copy_from_slice(&ty.to_le_bytes());

                // Length
                let input_len: u32 = 4 + 6;
                let content_len: u16 = 4 + input_len as u16;
                buffer[2..4].copy_from_slice(&content_len.to_le_bytes());

                // Input Len
                buffer[4..8].copy_from_slice(&input_len.to_be_bytes());

                // Input Ty
                let ty: u32 = SCROLL_MAGIC_GEN5;
                buffer[8..12].copy_from_slice(&ty.to_le_bytes());

                // Data
                buffer[12..14].copy_from_slice(&scroll_amount_1.to_le_bytes());
                buffer[14..16].copy_from_slice(&scroll_amount_2.to_le_bytes());
                buffer[16..18].copy_from_slice(&zero.to_le_bytes());

                Ok(4 + content_len as usize)
            }
            Self::MouseHorizontalScroll { scroll_amount } => {
                // Ty
                let ty = config.input_data;
                buffer[0..2].copy_from_slice(&ty.to_le_bytes());

                // Length
                let input_len: u32 = 4 + 2;
                let content_len: u16 = 4 + input_len as u16;
                buffer[2..4].copy_from_slice(&content_len.to_le_bytes());

                // Input Len
                buffer[4..8].copy_from_slice(&input_len.to_be_bytes());

                // Input Ty
                let ty: u32 = SS_HSCROLL_MAGIC;
                buffer[8..12].copy_from_slice(&ty.to_le_bytes());

                // Data
                buffer[12..14].copy_from_slice(&scroll_amount.to_le_bytes());

                Ok(4 + content_len as usize)
            }
            Self::Keyboard {
                action,
                flags,
                key_code,
                modifiers,
                zero,
            } => {
                // Ty
                let ty = config.input_data;
                buffer[0..2].copy_from_slice(&ty.to_le_bytes());

                // Length
                let input_len: u32 = 10;
                let content_len: u16 = 4 + input_len as u16;
                buffer[2..4].copy_from_slice(&content_len.to_le_bytes());

                // Input Len
                buffer[4..8].copy_from_slice(&input_len.to_be_bytes());

                // Input Ty
                let ty: u32 = match action {
                    KeyAction::Up => KEY_UP_EVENT_MAGIC,
                    KeyAction::Down => KEY_DOWN_EVENT_MAGIC,
                };
                buffer[8..12].copy_from_slice(&ty.to_le_bytes());

                // Data
                buffer[12..13].copy_from_slice(&[flags.bits() as u8]);
                buffer[13..15].copy_from_slice(&key_code.0.to_le_bytes());
                buffer[15..16].copy_from_slice(&[modifiers.bits() as u8]);
                buffer[16..18].copy_from_slice(&zero.to_le_bytes());

                Ok(4 + content_len as usize)
            }
            Self::Text { text, text_len } => {
                debug_assert!(*text_len <= text.len());
                if *text_len > text.len() {
                    return Err(ControlPacketNotSupported);
                }

                // Ty
                let ty = config.input_data;
                buffer[0..2].copy_from_slice(&ty.to_le_bytes());

                // Length
                // 4 = Input Ty
                let input_len: u32 = 4 + *text_len as u32;
                let content_len: u16 = 4 + input_len as u16;
                buffer[2..4].copy_from_slice(&content_len.to_le_bytes());

                // Input Len
                buffer[4..8].copy_from_slice(&input_len.to_be_bytes());

                // Input Ty
                let ty: u32 = UTF8_TEXT_EVENT_MAGIC;
                buffer[8..12].copy_from_slice(&ty.to_le_bytes());

                // Data
                buffer[12..(12 + *text_len)].copy_from_slice(&text[0..*text_len]);

                Ok(4 + content_len as usize)
            }
            Self::ControllerArrival {
                controller_number,
                ty: controller_type,
                capabilities,
                supported_buttons,
            } => {
                // See https://github.com/moonlight-stream/moonlight-common-c/blob/7b026e77be62175104640e7e722b758df6d3d0d7/src/InputStream.c#L1426-L1467

                // Ty
                let ty = config.input_data;
                buffer[0..2].copy_from_slice(&ty.to_le_bytes());

                // Length
                let input_len: u32 = 12;
                let content_len: u16 = 4 + input_len as u16;
                buffer[2..4].copy_from_slice(&content_len.to_le_bytes());

                // Input Len
                buffer[4..8].copy_from_slice(&input_len.to_be_bytes());

                // Input Ty
                let ty: u32 = SS_CONTROLLER_ARRIVAL_MAGIC;
                buffer[8..12].copy_from_slice(&ty.to_le_bytes());

                // Data
                buffer[12..13].copy_from_slice(&[*controller_number]);
                buffer[13..14].copy_from_slice(&[*controller_type as u8]);
                buffer[14..16].copy_from_slice(&capabilities.bits().to_le_bytes());
                buffer[16..20].copy_from_slice(&supported_buttons.bits().to_le_bytes());

                Ok(4 + content_len as usize)
            }
            Self::ControllerState {
                header_b,
                controller_number,
                active_gamepad_mask,
                mid_b,
                button_flags,
                left_trigger,
                right_trigger,
                left_stick_x,
                left_stick_y,
                right_stick_x,
                right_stick_y,
                tail_a,
                button_flags_2,
                tail_b,
            } => {
                // See https://github.com/moonlight-stream/moonlight-common-c/blob/7b026e77be62175104640e7e722b758df6d3d0d7/src/InputStream.c#L998-L1162
                // https://github.com/moonlight-stream/moonlight-common-c/blob/7b026e77be62175104640e7e722b758df6d3d0d7/src/Input.h#L93-L109
                // Note: we only support Gen 5 servers and above this makes this logic very easy

                // Ty
                let ty = config.input_data;
                buffer[0..2].copy_from_slice(&ty.to_le_bytes());

                // Length
                let input_len: u32 = 30;
                let content_len: u16 = 4 + input_len as u16;
                buffer[2..4].copy_from_slice(&content_len.to_le_bytes());

                // Input Len
                buffer[4..8].copy_from_slice(&input_len.to_be_bytes());

                // Input Ty
                let ty: u32 = MULTI_CONTROLLER_MAGIC_GEN5;
                buffer[8..12].copy_from_slice(&ty.to_le_bytes());

                // Data
                buffer[12..14].copy_from_slice(&header_b.to_le_bytes());
                buffer[14..16].copy_from_slice(&controller_number.to_le_bytes());
                buffer[16..18].copy_from_slice(&active_gamepad_mask.bits().to_le_bytes());
                buffer[18..20].copy_from_slice(&mid_b.to_le_bytes());
                buffer[20..22].copy_from_slice(&button_flags.to_le_bytes());
                buffer[22..23].copy_from_slice(&[*left_trigger]);
                buffer[23..24].copy_from_slice(&[*right_trigger]);
                buffer[24..26].copy_from_slice(&left_stick_x.to_le_bytes());
                buffer[26..28].copy_from_slice(&left_stick_y.to_le_bytes());
                buffer[28..30].copy_from_slice(&right_stick_x.to_le_bytes());
                buffer[30..32].copy_from_slice(&right_stick_y.to_le_bytes());
                buffer[32..34].copy_from_slice(&tail_a.to_le_bytes());
                buffer[34..36].copy_from_slice(&button_flags_2.to_le_bytes());
                buffer[36..38].copy_from_slice(&tail_b.to_le_bytes());

                Ok(4 + content_len as usize)
            }
            Self::ControllerBattery {
                controller_number,
                battery_state,
                battery_percentage,
                reserved,
            } => {
                // Ty
                let ty = config.input_data;
                buffer[0..2].copy_from_slice(&ty.to_le_bytes());

                // Length
                let input_len: u32 = 8;
                let content_len: u16 = 4 + input_len as u16;
                buffer[2..4].copy_from_slice(&content_len.to_le_bytes());

                // Input Len
                buffer[4..8].copy_from_slice(&input_len.to_be_bytes());

                // Input Ty
                let ty: u32 = SS_CONTROLLER_BATTERY_MAGIC;
                buffer[8..12].copy_from_slice(&ty.to_le_bytes());

                // Data
                buffer[12] = *controller_number;
                buffer[13] = *battery_state as u8;
                buffer[14] = *battery_percentage;
                buffer[15] = *reserved;

                Ok(4 + content_len as usize)
            }
            Self::ControllerMotion {
                controller_number,
                motion_type,
                reserved,
                x,
                y,
                z,
            } => {
                // See, this code also does the batching, but here we just want to serialize / deserialize
                // https://github.com/moonlight-stream/moonlight-common-c/blob/7b026e77be62175104640e7e722b758df6d3d0d7/src/InputStream.c#L1558-L1565
                // https://github.com/moonlight-stream/moonlight-common-c/blob/7b026e77be62175104640e7e722b758df6d3d0d7/src/InputStream.c#L511-L545

                // Ty
                let ty = config.input_data;
                buffer[0..2].copy_from_slice(&ty.to_le_bytes());

                // Length
                let input_len: u32 = 20;
                let content_len: u16 = 4 + input_len as u16;
                buffer[2..4].copy_from_slice(&content_len.to_le_bytes());

                // Input Len
                buffer[4..8].copy_from_slice(&input_len.to_be_bytes());

                // Input Ty
                let ty: u32 = SS_CONTROLLER_MOTION_MAGIC;
                buffer[8..12].copy_from_slice(&ty.to_le_bytes());

                // Data
                buffer[12] = *controller_number;
                buffer[13] = *motion_type as u8;
                buffer[14..16].copy_from_slice(reserved);
                buffer[16..20].copy_from_slice(&x.to_le_bytes());
                buffer[20..24].copy_from_slice(&y.to_le_bytes());
                buffer[24..28].copy_from_slice(&z.to_le_bytes());

                Ok(4 + content_len as usize)
            }
            Self::ControllerRumbleData {
                unused,
                controller_number,
                low_frequency,
                high_frequency,
            } => {
                todo!();
            }
            Self::ControllerRumbleTriggers { controller_number } => {
                todo!()
            }
            Self::ControllerSetLed {
                controller_number,
                r,
                g,
                b,
            } => {
                // Ty
                let Some(ty) = config.set_rgb_led else {
                    return Err(ControlPacketNotSupported);
                };
                buffer[0..2].copy_from_slice(&ty.to_le_bytes());

                // Length
                let content_len = 5u16;
                buffer[2..4].copy_from_slice(&content_len.to_le_bytes());

                buffer[4..6].copy_from_slice(&controller_number.to_le_bytes());
                buffer[6] = *r;
                buffer[7] = *g;
                buffer[8] = *b;

                Ok(4 + content_len as usize)
            }
            Self::ControllerSetMotion {
                controller_number,
                rate,
                motion_type,
            } => {
                // Ty
                let Some(ty) = config.set_motion_event else {
                    return Err(ControlPacketNotSupported);
                };
                buffer[0..2].copy_from_slice(&ty.to_le_bytes());

                // Length
                let content_len = 5u16;
                buffer[2..4].copy_from_slice(&content_len.to_le_bytes());

                buffer[4..6].copy_from_slice(&controller_number.to_le_bytes());
                buffer[6..8].copy_from_slice(&rate.to_le_bytes());
                buffer[8] = *motion_type as u8;

                Ok(4 + content_len as usize)
            }
            Self::Touch {
                event_type,
                reserved,
                rotation,
                pointer_id,
                x,
                y,
                pressure_or_distance,
                contact_area_minor,
                contact_area_major,
            } => {
                // See https://github.com/moonlight-stream/moonlight-common-c/blob/7b026e77be62175104640e7e722b758df6d3d0d7/src/InputStream.c#L1326-L1371

                // Ty
                let ty = config.input_data;
                buffer[0..2].copy_from_slice(&ty.to_le_bytes());

                // Length
                let input_len: u32 = 32;
                let content_len: u16 = 4 + input_len as u16;
                buffer[2..4].copy_from_slice(&content_len.to_le_bytes());

                // Input Len
                buffer[4..8].copy_from_slice(&input_len.to_be_bytes());

                // Input Ty
                let ty: u32 = SS_TOUCH_MAGIC;
                buffer[8..12].copy_from_slice(&ty.to_le_bytes());

                // Data
                buffer[12..13].copy_from_slice(&[*event_type as u8]);
                buffer[13..14].copy_from_slice(&[*reserved]);
                buffer[14..16].copy_from_slice(&rotation.to_le_bytes());
                buffer[16..20].copy_from_slice(&pointer_id.to_le_bytes());
                buffer[20..24].copy_from_slice(&x.to_le_bytes());
                buffer[24..28].copy_from_slice(&y.to_le_bytes());
                buffer[28..32].copy_from_slice(&pressure_or_distance.to_le_bytes());
                buffer[32..36].copy_from_slice(&contact_area_minor.to_le_bytes());
                buffer[36..40].copy_from_slice(&contact_area_major.to_le_bytes());

                Ok(4 + content_len as usize)
            }
            Self::Pen {
                event_type,
                tool_type,
                buttons,
                zero,
                x,
                y,
                pressure_or_distance,
                rotation,
                tilt,
                zero2,
                contact_area_minor,
                contact_area_major,
            } => {
                // See https://github.com/moonlight-stream/moonlight-common-c/blob/7b026e77be62175104640e7e722b758df6d3d0d7/src/InputStream.c#L1373-L1424

                // Ty
                let ty = config.input_data;
                buffer[0..2].copy_from_slice(&ty.to_le_bytes());

                // Length
                let input_len: u32 = 32;
                let content_len: u16 = 4 + input_len as u16;
                buffer[2..4].copy_from_slice(&content_len.to_le_bytes());

                // Input Len
                buffer[4..8].copy_from_slice(&input_len.to_be_bytes());

                // Input Ty
                let ty: u32 = SS_PEN_MAGIC;
                buffer[8..12].copy_from_slice(&ty.to_le_bytes());

                // Data
                buffer[12] = *event_type as u8;
                buffer[13] = *tool_type as u8;
                buffer[14] = buttons.bits();
                buffer[15] = *zero;
                buffer[16..20].copy_from_slice(&x.to_le_bytes());
                buffer[20..24].copy_from_slice(&y.to_le_bytes());
                buffer[24..28].copy_from_slice(&pressure_or_distance.to_le_bytes());
                buffer[28..30].copy_from_slice(&rotation.to_le_bytes());
                buffer[30] = *tilt;
                buffer[31] = *zero2;
                buffer[32..36].copy_from_slice(&contact_area_minor.to_le_bytes());
                buffer[36..40].copy_from_slice(&contact_area_major.to_le_bytes());

                Ok(4 + content_len as usize)
            }
            Self::ServerTermination { reason } => {
                let ty = config.server_termination.ok_or(ControlPacketNotSupported)?;
                buffer[0..2].copy_from_slice(&ty.to_le_bytes());

                match reason {
                    TerminationReason::Short(code) => {
                        let content_len: u16 = 2;
                        buffer[2..4].copy_from_slice(&content_len.to_le_bytes());

                        buffer[4..6].copy_from_slice(&code.to_be_bytes());

                        Ok(4 + content_len as usize)
                    }
                    TerminationReason::Long(code) => {
                        let content_len: u16 = 4;
                        buffer[2..4].copy_from_slice(&content_len.to_le_bytes());

                        buffer[4..8].copy_from_slice(&code.to_be_bytes());

                        Ok(4 + content_len as usize)
                    }
                }
            }
        }
    }

    // TODO: maybe replace option with an result?
    /// Payload is:
    /// - If not encrypted: the full payload
    /// - If encrypted: the decrypted payload
    #[instrument(level = Level::TRACE)]
    pub fn deserialize(
        packet_direction: PacketDirection,
        config: &ControlPacketConfig,
        payload: &[u8],
    ) -> Option<Self> {
        if payload.len() < 4 {
            warn!("Received packet that is too short (< 4 bytes)");
            return None;
        }
        let ty = u16::from_le_bytes([payload[0], payload[1]]);
        let len = u16::from_le_bytes([payload[2], payload[3]]);
        trace!("raw ty: {ty:#x}, len: {len}");

        let Some(ty) =
            ControlPacketType::deserialize(packet_direction, config, RawControlPacketType(ty))
        else {
            warn!("failed to deserialize ty: {ty:#x}");
            return None;
        };
        trace!("parsed type: {ty:?}");

        if payload.len() < 4 + len as usize - 1 {
            warn!(packet_ty = ?ty, full_len = payload.len(), got_len = payload.len() - 4, expected_len = len, "Received payload that has incorrect length in its length field");
            return None;
        }
        let payload = &payload[0..(4 + len as usize)];

        match ty {
            ControlPacketType::PeriodicPing => {
                // Moonlight says missing timestamp: https://github.com/moonlight-stream/moonlight-common-c/blob/2a5a1f3e8a57cbbb316ed7dfff3a3965c2e77d25/src/ControlStream.c#L1395-L1396
                // but Sunshine doesn't do anything: https://github.com/LizardByte/Sunshine/blob/0bbaa2db7c2ccececa696e11fb8c83e5f8a7f97d/src/stream.cpp#L923-L925
                Some(ControlPacket::PeriodicPing)
            }
            ControlPacketType::RequestIdr => Some(ControlPacket::RequestIdr),
            ControlPacketType::StartB => Some(ControlPacket::StartB),
            ControlPacketType::ControllerRumbleData => {
                todo!();
            }
            ControlPacketType::ControllerRumbleTriggers => {
                todo!()
            }
            ControlPacketType::ControllerSetMotion => {
                if payload.len() < 9 {
                    warn!("ControllerSetMotion packet too small");
                    return None;
                }

                let controller_number = u16::from_le_bytes([payload[4], payload[5]]);
                let rate = u16::from_le_bytes([payload[6], payload[7]]);
                let Some(motion_type) = MotionType::from_u8(payload[8]) else {
                    warn!(
                        controller_number = controller_number,
                        motion_type_raw = payload[8],
                        "received invalid motion type"
                    );

                    return None;
                };

                Some(ControlPacket::ControllerSetMotion {
                    controller_number,
                    rate,
                    motion_type,
                })
            }
            ControlPacketType::ControllerSetLed => {
                if payload.len() < 9 {
                    warn!("ControllerSetLed packet too small");
                    return None;
                }

                let controller_number = u16::from_le_bytes([payload[4], payload[5]]);
                let r = payload[6];
                let g = payload[7];
                let b = payload[8];

                Some(ControlPacket::ControllerSetLed {
                    controller_number,
                    r,
                    g,
                    b,
                })
            }
            ControlPacketType::Termination => {
                if payload.len() < 4 + 2 {
                    warn!("Termination packet too small");
                    return None;
                }

                if payload.len() >= 8 {
                    let code = u32::from_be_bytes([payload[4], payload[5], payload[6], payload[7]]);

                    Some(ControlPacket::ServerTermination {
                        reason: TerminationReason::Long(code),
                    })
                } else {
                    let code = u16::from_be_bytes([payload[4], payload[5]]);

                    Some(ControlPacket::ServerTermination {
                        reason: TerminationReason::Short(code),
                    })
                }
            }
            ControlPacketType::HdrMode => {
                // https://github.com/moonlight-stream/moonlight-common-c/blob/62687809b1f7410c3db4be2527503a54ae408d70/src/ControlStream.c#L1265-L1293
                if payload.len() < 4 + 1 {
                    warn!("HdrMode packet too small");
                    return None;
                }

                let enabled = payload[4] != 0;

                let mut sunshine = None;
                if config.server_version.is_sunshine_like() {
                    if payload.len() < 31 {
                        warn!(
                            "Received HdrMode packet from a sunshine server that doesn't contain the sunshine hdr extension."
                        );
                    } else {
                        let metadata = SunshineHdrMetadata {
                            display_primaries: [
                                Primary {
                                    x: u16::from_le_bytes([payload[5], payload[6]]),
                                    y: u16::from_le_bytes([payload[7], payload[8]]),
                                },
                                Primary {
                                    x: u16::from_le_bytes([payload[9], payload[10]]),
                                    y: u16::from_le_bytes([payload[11], payload[12]]),
                                },
                                Primary {
                                    x: u16::from_le_bytes([payload[13], payload[14]]),
                                    y: u16::from_le_bytes([payload[15], payload[16]]),
                                },
                            ],
                            white_point: Primary {
                                x: u16::from_le_bytes([payload[17], payload[18]]),
                                y: u16::from_le_bytes([payload[19], payload[20]]),
                            },
                            max_display_luminance: u16::from_le_bytes([payload[21], payload[22]]),
                            min_display_luminance: u16::from_le_bytes([payload[23], payload[24]]),
                            max_content_light_level: u16::from_le_bytes([payload[25], payload[26]]),
                            max_frame_average_light_level: u16::from_le_bytes([
                                payload[27],
                                payload[28],
                            ]),
                            max_full_frame_luminance: u16::from_le_bytes([
                                payload[29],
                                payload[30],
                            ]),
                        };

                        sunshine = Some(metadata);
                    }
                }

                Some(Self::HdrMode { enabled, sunshine })
            }
            ControlPacketType::LossStats => {
                if payload.len() < 4 + 32 {
                    warn!("LossStats packet too small");
                    return None;
                }

                let unknown1 = u32::from_le_bytes([payload[4], payload[5], payload[6], payload[7]]);
                let loss_report_interval_ms =
                    u32::from_le_bytes([payload[8], payload[9], payload[10], payload[11]]);
                let unknown2 =
                    u32::from_le_bytes([payload[12], payload[13], payload[14], payload[15]]);
                let last_good_frame = u64::from_le_bytes([
                    payload[16],
                    payload[17],
                    payload[18],
                    payload[19],
                    payload[20],
                    payload[21],
                    payload[22],
                    payload[23],
                ]);
                let unknown3 =
                    u32::from_le_bytes([payload[24], payload[25], payload[26], payload[27]]);
                let unknown4 =
                    u32::from_le_bytes([payload[28], payload[29], payload[30], payload[31]]);
                let unknown5 =
                    u32::from_le_bytes([payload[32], payload[33], payload[34], payload[35]]);

                Some(ControlPacket::LossStats {
                    unknown1,
                    loss_report_interval_ms,
                    unknown2,
                    last_good_frame,
                    unknown3,
                    unknown4,
                    unknown5,
                })
            }
            ControlPacketType::FrameStats => {
                warn!("FrameStats packet payload is unknown by this implementation");

                Some(ControlPacket::FrameStats {})
            }
            ControlPacketType::FrameFec => {
                if payload.len() < 4 + 21 {
                    warn!("FrameFec packet too small");
                    return None;
                }

                let frame_index =
                    u32::from_be_bytes([payload[4], payload[5], payload[6], payload[7]]);

                let highest_received_sequence_number = u16::from_be_bytes([payload[8], payload[9]]);

                let next_contiguous_sequence_number =
                    u16::from_be_bytes([payload[10], payload[11]]);

                let missing_packets_before_highest_received =
                    u16::from_be_bytes([payload[12], payload[13]]);

                let total_data_packets = u16::from_be_bytes([payload[14], payload[15]]);

                let total_parity_packets = u16::from_be_bytes([payload[16], payload[17]]);

                let received_data_packets = u16::from_be_bytes([payload[18], payload[19]]);

                let received_parity_packets = u16::from_be_bytes([payload[20], payload[21]]);

                let fec_percentage = payload[22];

                let multi_fec_block_index = payload[23];

                let multi_fec_block_count = payload[24];

                Some(ControlPacket::FrameFec {
                    frame_index,
                    highest_received_sequence_number,
                    next_contiguous_sequence_number,
                    missing_packets_before_highest_received,
                    total_data_packets,
                    total_parity_packets,
                    received_data_packets,
                    received_parity_packets,
                    fec_percentage,
                    multi_fec_block_index,
                    multi_fec_block_count,
                })
            }
            ControlPacketType::LongTermReferenceFrameAcknowledgement => {
                if payload.len() < 4 + 8 {
                    warn!("LTR ACK packet too small");
                    return None;
                }

                let frame_index =
                    u32::from_le_bytes([payload[4], payload[5], payload[6], payload[7]]);
                let reserved =
                    u32::from_le_bytes([payload[8], payload[9], payload[10], payload[11]]);

                Some(ControlPacket::LongTermReferenceFrameAcknowledgement {
                    frame_index,
                    reserved,
                })
            }
            ControlPacketType::InvalidateReferenceFrames => {
                if payload.len() < 4 + 24 {
                    warn!("InvalidateReferenceFrames packet too small");
                    return None;
                }

                let first_frame_index =
                    u32::from_le_bytes([payload[4], payload[5], payload[6], payload[7]]);
                let reserved1 =
                    u32::from_le_bytes([payload[8], payload[9], payload[10], payload[11]]);
                let last_frame_index =
                    u32::from_le_bytes([payload[12], payload[13], payload[14], payload[15]]);

                let mut reserved2 = [0u32; 3];
                for i in 0..3 {
                    let start = 16 + i * 4;
                    reserved2[i] = u32::from_le_bytes([
                        payload[start],
                        payload[start + 1],
                        payload[start + 2],
                        payload[start + 3],
                    ]);
                }

                Some(ControlPacket::InvalidateReferenceFrames {
                    first_frame_index,
                    reserved1,
                    last_frame_index,
                    reserved2,
                })
            }
            ControlPacketType::InputData => {
                if payload.len() < 4 + 8 {
                    warn!("InputData packet too small");
                    return None;
                }

                let input_len =
                    u32::from_be_bytes([payload[4], payload[5], payload[6], payload[7]]);
                let input_ty =
                    u32::from_le_bytes([payload[8], payload[9], payload[10], payload[11]]);

                // Control Header + Input Len + The rest
                if payload.len() < 4 + 4 + input_len as usize {
                    warn!(actual_payload_len = ?payload.len(), packet_header_len = ?len, input_header_len = ?input_len, "InputData length is bigger than the payload length");
                    return None;
                }

                match input_ty {
                    MOUSE_MOVE_REL_MAGIC_GEN5 => {
                        if input_len < 8 {
                            warn!(input_len = ?input_len, "MouseMoveRelative packet too small!");
                            None
                        } else {
                            let delta_x = i16::from_be_bytes([payload[12], payload[13]]);
                            let delta_y = i16::from_be_bytes([payload[14], payload[15]]);

                            Some(ControlPacket::MouseMoveRelative { delta_x, delta_y })
                        }
                    }
                    MOUSE_MOVE_ABS_MAGIC => {
                        if input_len < 14 {
                            warn!(input_len = ?input_len, "MouseMoveAbsolute packet too small!");
                            None
                        } else {
                            let x = i16::from_be_bytes([payload[12], payload[13]]);
                            let y = i16::from_be_bytes([payload[14], payload[15]]);
                            let unused = i16::from_be_bytes([payload[16], payload[17]]);
                            let reference_width = i16::from_be_bytes([payload[18], payload[19]]);
                            let reference_height = i16::from_be_bytes([payload[20], payload[21]]);

                            Some(ControlPacket::MouseMoveAbsolute {
                                x,
                                y,
                                unused,
                                reference_width,
                                reference_height,
                            })
                        }
                    }
                    MOUSE_BUTTON_DOWN_EVENT_MAGIC_GEN5 | MOUSE_BUTTON_UP_EVENT_MAGIC_GEN5 => {
                        if input_len < 5 {
                            warn!(input_len = ?input_len, "MouseButton packet too small!");
                            None
                        } else {
                            let action = match input_ty {
                                MOUSE_BUTTON_DOWN_EVENT_MAGIC_GEN5 => MouseButtonAction::Press,
                                MOUSE_BUTTON_UP_EVENT_MAGIC_GEN5 => MouseButtonAction::Release,
                                _ => unreachable!(),
                            };

                            let button = u8::from_be_bytes([payload[12]]);
                            let Some(button) = MouseButton::from_u8(button) else {
                                warn!(mouse_button_raw = ?button, "Received invalid mouse button");
                                return None;
                            };

                            Some(ControlPacket::MouseButton { action, button })
                        }
                    }
                    SCROLL_MAGIC_GEN5 => {
                        if input_len < 4 + 6 {
                            warn!(input_len = ?input_len, "MouseScroll packet too small!");
                            None
                        } else {
                            let scroll_amount_1 = i16::from_le_bytes([payload[12], payload[13]]);
                            let scroll_amount_2 = i16::from_le_bytes([payload[14], payload[15]]);
                            let zero = i16::from_le_bytes([payload[16], payload[17]]);

                            Some(ControlPacket::MouseScroll {
                                scroll_amount_1,
                                scroll_amount_2,
                                zero,
                            })
                        }
                    }
                    SS_HSCROLL_MAGIC => {
                        if input_len < 4 + 2 {
                            warn!(input_len = ?input_len, "MouseHorizontalScroll packet too small!");
                            None
                        } else {
                            let scroll_amount = i16::from_le_bytes([payload[12], payload[13]]);

                            Some(ControlPacket::MouseHorizontalScroll { scroll_amount })
                        }
                    }
                    KEY_DOWN_EVENT_MAGIC | KEY_UP_EVENT_MAGIC => {
                        if input_len < 10 {
                            warn!(input_len = ?input_len, "Key packet too small!");
                            None
                        } else {
                            let action = match input_ty {
                                KEY_DOWN_EVENT_MAGIC => KeyAction::Down,
                                KEY_UP_EVENT_MAGIC => KeyAction::Up,
                                _ => unreachable!(),
                            };

                            let flags = KeyFlags::from_bits_retain(payload[12] as i8);
                            let key_code = KeyCode(i16::from_le_bytes([payload[13], payload[14]]));
                            let modifiers = KeyModifiers::from_bits_retain(payload[15] as i8);
                            let zero = i16::from_le_bytes([payload[16], payload[17]]);

                            Some(ControlPacket::Keyboard {
                                action,
                                flags,
                                key_code,
                                modifiers,
                                zero,
                            })
                        }
                    }
                    UTF8_TEXT_EVENT_MAGIC => {
                        let mut text = [0; _];

                        let mut text_len = input_len as usize - 4;
                        let text_ref = &payload[12..(12 + text_len)];
                        if text_len >= text.len() {
                            warn!(
                                got_len = text_len,
                                max_len = text.len(),
                                "UTF8 Text packet was too large, shortening to {}",
                                text.len()
                            );
                            text_len = text.len();
                        }
                        text[0..text_len].copy_from_slice(text_ref);

                        Some(ControlPacket::Text { text, text_len })
                    }
                    SS_TOUCH_MAGIC => {
                        if input_len < 4 + 28 {
                            warn!(input_len = ?input_len, "Touch packet too small!");
                            None
                        } else {
                            let Some(event_type) = TouchEventType::from_u8(payload[12]) else {
                                warn!(
                                    got_type = payload[12],
                                    "Touch packet contains unknown touch event type"
                                );
                                return None;
                            };
                            let reserved = payload[13];
                            let rotation = u16::from_le_bytes([payload[14], payload[15]]);
                            let pointer_id = u32::from_le_bytes([
                                payload[16],
                                payload[17],
                                payload[18],
                                payload[19],
                            ]);
                            let x = f32::from_le_bytes([
                                payload[20],
                                payload[21],
                                payload[22],
                                payload[23],
                            ]);
                            let y = f32::from_le_bytes([
                                payload[24],
                                payload[25],
                                payload[26],
                                payload[27],
                            ]);
                            let pressure_or_distance = f32::from_le_bytes([
                                payload[28],
                                payload[29],
                                payload[30],
                                payload[31],
                            ]);
                            let contact_area_minor = f32::from_le_bytes([
                                payload[32],
                                payload[33],
                                payload[34],
                                payload[35],
                            ]);
                            let contact_area_major = f32::from_le_bytes([
                                payload[36],
                                payload[37],
                                payload[38],
                                payload[39],
                            ]);

                            Some(ControlPacket::Touch {
                                event_type,
                                reserved,
                                rotation,
                                pointer_id,
                                x,
                                y,
                                pressure_or_distance,
                                contact_area_minor,
                                contact_area_major,
                            })
                        }
                    }
                    SS_CONTROLLER_ARRIVAL_MAGIC => {
                        if input_len < 4 + 8 {
                            warn!(input_len = ?input_len, "ControllerArrival packet too small!");
                            None
                        } else {
                            let controller_number = payload[12];
                            let ty = ControllerType::from_u8(payload[13])
                                .unwrap_or(ControllerType::Unknown);
                            let capabilities =
                                ControllerCapabilities::from_bits_retain(u16::from_le_bytes([
                                    payload[14],
                                    payload[15],
                                ]));
                            let supported_buttons =
                                ControllerButtons::from_bits_retain(u32::from_le_bytes([
                                    payload[16],
                                    payload[17],
                                    payload[18],
                                    payload[19],
                                ]));

                            Some(ControlPacket::ControllerArrival {
                                controller_number,
                                ty,
                                capabilities,
                                supported_buttons,
                            })
                        }
                    }
                    MULTI_CONTROLLER_MAGIC_GEN5 => {
                        if input_len < 30 {
                            warn!(input_len = ?input_len, "ControllerArrival packet too small!");
                            None
                        } else {
                            let header_b = i16::from_le_bytes([payload[12], payload[13]]);
                            let controller_number = i16::from_le_bytes([payload[14], payload[15]]);
                            let active_gamepad_mask =
                                ActiveGamepads::from_bits_retain(u16::from_le_bytes([
                                    payload[16],
                                    payload[17],
                                ]));
                            let mid_b = i16::from_le_bytes([payload[18], payload[19]]);
                            let button_flags = i16::from_le_bytes([payload[20], payload[21]]);
                            let left_trigger = payload[22];
                            let right_trigger = payload[23];
                            let left_stick_x = i16::from_le_bytes([payload[24], payload[25]]);
                            let left_stick_y = i16::from_le_bytes([payload[26], payload[27]]);
                            let right_stick_x = i16::from_le_bytes([payload[28], payload[29]]);
                            let right_stick_y = i16::from_le_bytes([payload[30], payload[31]]);
                            let tail_a = i16::from_le_bytes([payload[32], payload[33]]);
                            let button_flags_2 = i16::from_le_bytes([payload[34], payload[35]]);
                            let tail_b = i16::from_le_bytes([payload[36], payload[37]]);

                            Some(ControlPacket::ControllerState {
                                header_b,
                                controller_number,
                                active_gamepad_mask,
                                mid_b,
                                button_flags,
                                left_trigger,
                                right_trigger,
                                left_stick_x,
                                left_stick_y,
                                right_stick_x,
                                right_stick_y,
                                tail_a,
                                button_flags_2,
                                tail_b,
                            })
                        }
                    }
                    SS_CONTROLLER_BATTERY_MAGIC => {
                        if input_len < 4 {
                            warn!(input_len = ?input_len, "ControllerBattery packet too small!");
                            None
                        } else {
                            let controller_number = payload[12];
                            let battery_state =
                                BatteryState::from_u8(payload[13]).unwrap_or_else(|| {
                                    warn!(
                                        controller_number = controller_number,
                                        battery_state_raw = payload[13],
                                        "received unknown controller battery state"
                                    );
                                    BatteryState::Unknown
                                });
                            let battery_percentage = payload[14];
                            let reserved = payload[15];

                            Some(ControlPacket::ControllerBattery {
                                controller_number,
                                battery_state,
                                battery_percentage,
                                reserved,
                            })
                        }
                    }
                    SS_CONTROLLER_MOTION_MAGIC => {
                        if input_len < 20 {
                            warn!(input_len = ?input_len, "ControllerMotion packet too small!");
                            None
                        } else {
                            let controller_number = payload[12];
                            let Some(motion_type) = MotionType::from_u8(payload[13]) else {
                                warn!(
                                    controller_number = controller_number,
                                    motion_type_raw = payload[13],
                                    "received invalid motion type"
                                );
                                return None;
                            };
                            let reserved = [payload[14], payload[15]];
                            let x = f32::from_le_bytes([
                                payload[16],
                                payload[17],
                                payload[18],
                                payload[19],
                            ]);
                            let y = f32::from_le_bytes([
                                payload[20],
                                payload[21],
                                payload[22],
                                payload[23],
                            ]);
                            let z = f32::from_le_bytes([
                                payload[24],
                                payload[25],
                                payload[26],
                                payload[27],
                            ]);

                            Some(ControlPacket::ControllerMotion {
                                controller_number,
                                motion_type,
                                reserved,
                                x,
                                y,
                                z,
                            })
                        }
                    }
                    SS_PEN_MAGIC => {
                        if input_len < 32 {
                            warn!(input_len = ?input_len, "Pen packet too small!");
                            None
                        } else {
                            let Some(event_type) = TouchEventType::from_u8(payload[12]) else {
                                warn!(
                                    got_type = payload[12],
                                    "Pen packet contains unknown touch event type"
                                );
                                return None;
                            };

                            let tool_type = ToolType::from_u8(payload[13]).unwrap_or_else(|| {
                                warn!("Pen packet contains unknown tool type, continuing with Unknown tool type");
                                ToolType::Unknown
                            });

                            let buttons = PenButtons::from_bits_retain(payload[14]);
                            let zero = payload[15];
                            let x = f32::from_le_bytes([
                                payload[16],
                                payload[17],
                                payload[18],
                                payload[19],
                            ]);
                            let y = f32::from_le_bytes([
                                payload[20],
                                payload[21],
                                payload[22],
                                payload[23],
                            ]);
                            let pressure_or_distance = f32::from_le_bytes([
                                payload[24],
                                payload[25],
                                payload[26],
                                payload[27],
                            ]);
                            let rotation = u16::from_le_bytes([payload[28], payload[29]]);
                            let tilt = payload[30];
                            let zero2 = payload[31];
                            let contact_area_minor = f32::from_le_bytes([
                                payload[32],
                                payload[33],
                                payload[34],
                                payload[35],
                            ]);
                            let contact_area_major = f32::from_le_bytes([
                                payload[36],
                                payload[37],
                                payload[38],
                                payload[39],
                            ]);

                            Some(ControlPacket::Pen {
                                event_type,
                                tool_type,
                                buttons,
                                zero,
                                x,
                                y,
                                pressure_or_distance,
                                rotation,
                                tilt,
                                zero2,
                                contact_area_minor,
                                contact_area_major,
                            })
                        }
                    }
                    _ => {
                        warn!("InputData packet contains not known input type: {input_ty:#}");
                        None
                    }
                }
            }
            ControlPacketType::SetAdaptiveTriggers => {
                todo!()
            }
        }
    }
}
