use std::{ops::Deref, time::Duration};

use num::FromPrimitive;
use thiserror::Error;
use tracing::{Level, instrument, trace, warn};

use crate::{
    ServerVersion,
    stream::{
        control::{KeyAction, KeyCode, KeyFlags, KeyModifiers, MouseButton, MouseButtonAction},
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

// TODO: use this struct for the enet channel
pub enum EnetChannel {}

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
    /// - [ControlPacket::Termination]
    pub termination: Option<RawControlPacketType>,
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
                termination: None,
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
                termination: Some(RawControlPacketType(0x0109)),
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
                termination: Some(RawControlPacketType(0x0100)),
                hdr_mode: Some(RawControlPacketType(0x010e)),

                frame_fec: server_version
                    .is_sunshine_like()
                    .then_some(RawControlPacketType(0x5502)),

                rumble_triggers: None,
                set_motion_event: None,
                set_rgb_led: None,
                set_adaptive_triggers: None,
            }),

            // TODO: don't panic
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
    RumbleData,
    Termination,
    HdrMode,
    InputData,
    FrameFec,
    RumbleTriggers,
    SetMotionEvent,
    SetRgbLed,
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
            Self::RumbleData => PacketDirection::ServerBound,
            Self::Termination => PacketDirection::ServerBound,
            Self::HdrMode => PacketDirection::ClientBound,
            Self::InputData => PacketDirection::ServerBound,
            Self::FrameFec => PacketDirection::ServerBound,
            Self::RumbleTriggers => PacketDirection::ServerBound,
            Self::SetMotionEvent => PacketDirection::ClientBound,
            Self::SetRgbLed => PacketDirection::ClientBound,
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
            ControlPacketType::RumbleData => config.rumble_data,
            ControlPacketType::Termination => config.termination,
            ControlPacketType::HdrMode => config.hdr_mode,
            ControlPacketType::InputData => Some(config.input_data),
            ControlPacketType::FrameFec => config.frame_fec,
            ControlPacketType::RumbleTriggers => config.rumble_triggers,
            ControlPacketType::SetMotionEvent => config.set_motion_event,
            ControlPacketType::SetRgbLed => config.set_rgb_led,
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

#[derive(Debug, PartialEq)]
pub enum ControlPacket {
    // -- Server Sent Events
    // TODO: are those be or le
    RumbleData {
        // TODO: does unused exist?
        unused: u16,
        controller_id: u16,
        low_frequency: u16,
        high_frequency: u16,
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
    MouseMoveRelative {
        delta_x: i16,
        delta_y: i16,
    },
    /// Moves the mouse to x and y based on the reference width and height
    ///
    /// References:
    /// - https://github.com/games-on-whales/wolf/blob/5a393daafac36ff86453504d96faea50d160780d/src/moonlight-protocol/moonlight/control.hpp#L135-L141
    MouseMoveAbsolute {
        x: i16,
        y: i16,
        unused: i16,
        reference_width: i16,
        reference_height: i16,
    },
    /// References:
    /// - https://games-on-whales.github.io/wolf/stable/protocols/input-data.html#_mouse_button
    /// - https://github.com/games-on-whales/wolf/blob/5a393daafac36ff86453504d96faea50d160780d/src/moonlight-protocol/moonlight/control.hpp#L143-L145
    MouseButton {
        action: MouseButtonAction,
        button: MouseButton,
    },
    /// Sends a keyboard event to the host.
    ///
    /// References:
    /// - https://games-on-whales.github.io/wolf/stable/protocols/input-data.html#_keyboard
    /// - https://github.com/games-on-whales/wolf/blob/5a393daafac36ff86453504d96faea50d160780d/src/moonlight-protocol/moonlight/control.hpp#L157-L162
    Keyboard {
        action: KeyAction,
        flags: KeyFlags,
        key_code: KeyCode,
        modifier: KeyModifiers,
        zero: i16,
    },
    /// Vertical Scrolling.
    ///
    /// Only use scroll_amount_1.
    ///
    /// References:
    /// - https://games-on-whales.github.io/wolf/stable/protocols/input-data.html#_mouse_scroll
    /// - https://github.com/games-on-whales/wolf/blob/5a393daafac36ff86453504d96faea50d160780d/src/moonlight-protocol/moonlight/control.hpp#L147-L151
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
    MouseHorizontalScroll {
        amount: i16,
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
    // TODO: touch, controller, pen
}

impl ControlPacket {
    // TODO: what is the max size
    /// This is the maximum size a packet can have
    pub const MAX_SIZE: usize = 36;

    pub fn ty(&self) -> ControlPacketType {
        // TODO: fully implement
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
            Self::RumbleData { .. } => ControlPacketType::RumbleData,
            // Self::Termination => ControlPacketType::Termination,
            Self::HdrMode { .. } => ControlPacketType::HdrMode,
            Self::MouseButton { .. } => ControlPacketType::InputData,
            Self::MouseMoveRelative { .. } => ControlPacketType::InputData,
            Self::MouseMoveAbsolute { .. } => ControlPacketType::InputData,
            Self::MouseScroll { .. } => ControlPacketType::InputData,
            Self::MouseHorizontalScroll { .. } => ControlPacketType::InputData,
            Self::Keyboard { .. } => ControlPacketType::InputData,
            // Self::RumbleTriggers => ControlPacketType::RumbleTriggers,
            // Self::SetMotionEvent => ControlPacketType::SetMotionEvent,
            // Self::SetRgbLed => ControlPacketType::SetRgbLed,
            // Self::SetAdaptiveTriggers => ControlPacketType::SetAdaptiveTriggers,
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
                let ty: u32 = 0x00000007;
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
                let ty: u32 = 0x00000005;
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
                    MouseButtonAction::Press => 0x00000008,
                    MouseButtonAction::Release => 0x00000009,
                };
                buffer[8..12].copy_from_slice(&ty.to_le_bytes());

                // Data
                buffer[12..13].copy_from_slice(&[*button as u8]);

                Ok(4 + content_len as usize)
            }
            Self::Keyboard {
                action,
                flags,
                key_code,
                modifier,
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
                    KeyAction::Up => 0x00000004,
                    KeyAction::Down => 0x00000003,
                };
                buffer[8..12].copy_from_slice(&ty.to_le_bytes());

                // Data
                buffer[12..13].copy_from_slice(&[flags.bits() as u8]);
                buffer[13..15].copy_from_slice(&key_code.0.to_le_bytes());
                buffer[15..16].copy_from_slice(&[modifier.bits() as u8]);
                buffer[16..18].copy_from_slice(&zero.to_le_bytes());

                Ok(4 + content_len as usize)
            }
            _ => todo!(),
        }
        // TODO
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
            ControlPacketType::RumbleData => {
                todo!();
            }
            ControlPacketType::RumbleTriggers => {
                todo!()
            }
            ControlPacketType::SetMotionEvent => {
                todo!()
            }
            ControlPacketType::SetRgbLed => {
                todo!()
            }
            ControlPacketType::Termination => {
                // https://github.com/moonlight-stream/moonlight-common-c/blob/435bc6a5a4852c90cfb037de1378c0334ed36d8e/src/ControlStream.c#L1241-L1269
                todo!()
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
                    0x00000007 => {
                        if input_len < 8 {
                            warn!(input_len = ?input_len, "MouseMoveRelative packet too small!");
                            None
                        } else {
                            let delta_x = i16::from_be_bytes([payload[12], payload[13]]);
                            let delta_y = i16::from_be_bytes([payload[14], payload[15]]);

                            Some(ControlPacket::MouseMoveRelative { delta_x, delta_y })
                        }
                    }
                    0x00000005 => {
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
                    0x00000008 | 0x00000009 => {
                        if input_len < 5 {
                            warn!(input_len = ?input_len, "MouseButton packet too small!");
                            None
                        } else {
                            let action = match input_ty {
                                0x00000008 => MouseButtonAction::Press,
                                0x00000009 => MouseButtonAction::Release,
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
                    0x00000003 | 0x00000004 => {
                        if input_len < 10 {
                            warn!(input_len = ?input_len, "Key packet too small!");
                            None
                        } else {
                            let action = match input_ty {
                                0x00000003 => KeyAction::Down,
                                0x00000004 => KeyAction::Up,
                                _ => unreachable!(),
                            };

                            let flags = KeyFlags::from_bits_retain(payload[12] as i8);
                            let key_code = KeyCode(i16::from_le_bytes([payload[13], payload[14]]));
                            let modifier = KeyModifiers::from_bits_retain(payload[15] as i8);
                            let zero = i16::from_le_bytes([payload[16], payload[17]]);

                            Some(ControlPacket::Keyboard {
                                action,
                                flags,
                                key_code,
                                modifier,
                                zero,
                            })
                        }
                    }
                    _ => {
                        warn!("InputData packet contains not known input type: {input_ty:#}");
                        None
                    }
                }
            }
            _ => todo!(),
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod test {
    // TODO: test that all ControlPacketType types serialize and deserialize to their correct types

    use crate::{
        ServerVersion, init_test,
        stream::{
            control::{KeyAction, KeyCode, KeyFlags, KeyModifiers, MouseButton, MouseButtonAction},
            proto::control::packet::{
                ControlPacket, ControlPacketConfig, ControlPacketType,
                ENCRYPTED_CONTROL_PACKET_AES_GCM_TAG_LENGTH, ENCRYPTED_CONTROL_PACKET_TYPE,
                EncryptedControlHeader, PacketDirection,
            },
            video::{Primary, SunshineHdrMetadata},
        },
    };

    #[test]
    fn test_encrypted_control_header_serialization() {
        let assert_eq_header =
            |deserialized: EncryptedControlHeader,
             serialized: [u8; EncryptedControlHeader::SIZE]| {
                let mut buffer = [0; EncryptedControlHeader::SIZE];
                deserialized.serialize(&mut buffer);

                assert_eq!(buffer, serialized);
                assert_eq!(EncryptedControlHeader::deserialize(&buffer), deserialized);
            };

        assert_eq_header(
            EncryptedControlHeader {
                ty: ENCRYPTED_CONTROL_PACKET_TYPE,
                len: 0x1234,
                sequence_number: 0xABCD,
                tag: [0x11; ENCRYPTED_CONTROL_PACKET_AES_GCM_TAG_LENGTH],
            },
            [
                // ty (LE)
                0x01, 0x00, // len (LE)
                0x34, 0x12, // sequence_number (LE, u32!)
                0xCD, 0xAB, 0x00, 0x00, // tag
                0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11,
                0x11, 0x11,
            ],
        );

        assert_eq_header(
            EncryptedControlHeader {
                ty: ENCRYPTED_CONTROL_PACKET_TYPE,
                len: 1,
                sequence_number: 2,
                tag: [
                    0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF, 0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66,
                    0x77, 0x88, 0x99,
                ],
            },
            [
                // ty (LE)
                0x01, 0x00, // len (LE)
                0x01, 0x00, // sequence_number (LE, u32!)
                0x02, 0x00, 0x00, 0x00, // tag
                0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF, 0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77,
                0x88, 0x99,
            ],
        );
    }

    fn sunshine_gen_7_config() -> ControlPacketConfig {
        ControlPacketConfig::new(ServerVersion::new(7, 1, 431, -1), false).unwrap()
    }

    #[test]
    fn packet_directions() {
        // Make sure all packet directions for Ty::direction and Ty::deserialize match

        const PACKET_TYPES: &[ControlPacketType] = &[
            ControlPacketType::PeriodicPing,
            ControlPacketType::RequestIdr,
            ControlPacketType::StartB,
            ControlPacketType::InvalidateReferenceFrames,
            ControlPacketType::LongTermReferenceFrameAcknowledgement,
            ControlPacketType::LossStats,
            ControlPacketType::FrameStats,
            ControlPacketType::RumbleData,
            ControlPacketType::Termination,
            ControlPacketType::HdrMode,
            ControlPacketType::InputData,
            ControlPacketType::FrameFec,
            ControlPacketType::RumbleTriggers,
            ControlPacketType::SetMotionEvent,
            ControlPacketType::SetRgbLed,
            ControlPacketType::SetAdaptiveTriggers,
        ];

        let config = sunshine_gen_7_config();

        for ty in PACKET_TYPES {
            assert_eq!(
                ControlPacketType::deserialize(
                    ty.direction(),
                    &config,
                    ty.serialize(&config).unwrap()
                ),
                Some(*ty)
            );
        }
    }

    fn test_packet(
        direction: PacketDirection,
        config: ControlPacketConfig,
        expected_packet: ControlPacket,
        expected_bytes: &[u8],
    ) {
        let mut bytes = [0; _];
        let len = expected_packet.serialize(&config, &mut bytes).unwrap();
        let bytes = &bytes[0..len];
        assert_eq!(bytes, expected_bytes, "Serialize: {:?}", expected_packet);

        let packet = ControlPacket::deserialize(direction, &config, bytes).unwrap();
        assert_eq!(
            packet, expected_packet,
            "Deserialize: {:?}",
            expected_packet
        );
    }

    #[test]
    fn ping() {
        test_packet(
            PacketDirection::ServerBound,
            sunshine_gen_7_config(),
            ControlPacket::PeriodicPing,
            &[0, 2, 4, 0, 0, 0, 0, 0],
        );
    }

    #[test]
    fn hdr_mode() {
        init_test!();

        test_packet(
            PacketDirection::ClientBound,
            sunshine_gen_7_config(),
            ControlPacket::HdrMode {
                enabled: false,
                sunshine: None,
            },
            &[14, 1, 1, 0, 0],
        );

        test_packet(
            PacketDirection::ClientBound,
            sunshine_gen_7_config(),
            ControlPacket::HdrMode {
                enabled: true,
                sunshine: None,
            },
            &[14, 1, 1, 0, 1],
        );
    }
    #[test]
    fn hdr_mode_sunshine() {
        test_packet(
            PacketDirection::ClientBound,
            sunshine_gen_7_config(),
            ControlPacket::HdrMode {
                enabled: true,
                sunshine: Some(SunshineHdrMetadata {
                    display_primaries: [
                        Primary { x: 34000, y: 16000 }, // Red
                        Primary { x: 13250, y: 34500 }, // Green
                        Primary { x: 7500, y: 3000 },   // Blue
                    ],
                    white_point: Primary { x: 15635, y: 16450 },
                    max_display_luminance: 1000,
                    min_display_luminance: 50,
                    max_content_light_level: 1000,
                    max_frame_average_light_level: 400,
                    max_full_frame_luminance: 600,
                }),
            },
            &[
                14, 1, // Ty
                27, 0,    // Len
                0x01, // HDR enabled
                // Display Primaries
                0xD0, 0x84, // R.x = 34000
                0x80, 0x3E, // R.y = 16000
                0xC2, 0x33, // G.x = 13250
                0xC4, 0x86, // G.y = 34500
                0x4C, 0x1D, // B.x = 7500
                0xB8, 0x0B, // B.y = 3000
                // White point
                0x13, 0x3D, // x = 15635
                0x42, 0x40, // y = 16450
                // Luminance values
                0xE8, 0x03, // maxDisplayLuminance = 1000
                0x32, 0x00, // minDisplayLuminance = 50
                0xE8, 0x03, // maxContentLightLevel = 1000
                0x90, 0x01, // maxFrameAverageLightLevel = 400
                0x58, 0x02, // maxFullFrameLuminance = 600
            ],
        );
    }

    #[test]
    fn request_idr() {
        test_packet(
            PacketDirection::ServerBound,
            sunshine_gen_7_config(),
            ControlPacket::RequestIdr,
            &[5, 3, 2, 0, 0, 0],
        );
    }

    #[test]
    fn start_b() {
        test_packet(
            PacketDirection::ServerBound,
            sunshine_gen_7_config(),
            ControlPacket::StartB,
            &[7, 3, 1, 0, 0],
        );
    }

    #[test]
    fn loss_stats() {
        test_packet(
            PacketDirection::ServerBound,
            sunshine_gen_7_config(),
            ControlPacket::LossStats {
                unknown1: 0,
                loss_report_interval_ms: 500,
                unknown2: 1000,
                last_good_frame: 1,
                unknown3: 0,
                unknown4: 0,
                unknown5: 0x14,
            },
            &[
                1, 2, // Type
                32, 0, // Length = 32
                0, 0, 0, 0, 244, 1, 0, 0, 232, 3, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 20, 0, 0, 0,
            ],
        );
    }

    #[test]
    fn frame_fec() {
        test_packet(
            PacketDirection::ServerBound,
            sunshine_gen_7_config(),
            ControlPacket::FrameFec {
                frame_index: 42,
                highest_received_sequence_number: 1200,
                next_contiguous_sequence_number: 1180,
                missing_packets_before_highest_received: 20,
                total_data_packets: 100,
                total_parity_packets: 10,
                received_data_packets: 95,
                received_parity_packets: 8,
                fec_percentage: 10,
                multi_fec_block_index: 0,
                multi_fec_block_count: 1,
            },
            &[
                2, 85, // Type
                21, 0x00, // Length = 21 (LE)
                0x00, 0x00, 0x00, 0x2A, // frame_index = 42 (BE)
                0x04, 0xB0, // highest_received_sequence_number = 1200
                0x04, 0x9C, // next_contiguous_sequence_number = 1180
                0x00, 0x14, // missing_packets_before_highest_received = 20
                0x00, 0x64, // total_data_packets = 100
                0x00, 0x0A, // total_parity_packets = 10
                0x00, 0x5F, // received_data_packets = 95
                0x00, 0x08, // received_parity_packets = 8
                0x0A, // fec_percentage = 10
                0x00, // multi_fec_block_index = 0
                0x01, // multi_fec_block_count = 1
            ],
        );
    }

    #[test]
    fn invalidate_reference_frames() {
        test_packet(
            PacketDirection::ServerBound,
            sunshine_gen_7_config(),
            ControlPacket::InvalidateReferenceFrames {
                first_frame_index: 1,
                reserved1: 0,
                last_frame_index: 2,
                reserved2: [0, 0, 0],
            },
            &[
                0x01, 0x03, // Ty (0x0301 LE)
                24, 0x00, // Len
                1, 0, 0, 0, // first
                0, 0, 0, 0, // reserved1
                2, 0, 0, 0, // last
                0, 0, 0, 0, // reserved2[0]
                0, 0, 0, 0, // reserved2[1]
                0, 0, 0, 0, // reserved2[2]
            ],
        );
    }

    #[test]
    fn long_term_reference_frame_acknowledgement() {
        test_packet(
            PacketDirection::ServerBound,
            sunshine_gen_7_config(),
            ControlPacket::LongTermReferenceFrameAcknowledgement {
                frame_index: 42,
                reserved: 0,
            },
            &[
                0x50, 0x03, // Ty (0x0350 LE)
                8, 0x00, // Len
                42, 0, 0, 0, 0, 0, 0, 0,
            ],
        );
    }

    #[test]
    fn mouse_move_relative() {
        test_packet(
            PacketDirection::ServerBound,
            sunshine_gen_7_config(),
            ControlPacket::MouseMoveRelative {
                delta_x: 1,
                delta_y: 0,
            },
            &[
                0x06, 0x02, // Ty
                0x0c, 0x00, // Len
                0x00, 0x00, 0x00, 0x08, // Input Len
                0x07, 0x00, 0x00, 0x00, // Input Ty
                0x00, 0x01, // Delta X
                0x00, 0x00, // Delta Y
            ],
        );
    }

    #[test]
    fn mouse_move_absolute() {
        test_packet(
            PacketDirection::ServerBound,
            sunshine_gen_7_config(),
            ControlPacket::MouseMoveAbsolute {
                x: 0,
                y: 1,
                unused: 0,
                reference_width: 1000,
                reference_height: 1000,
            },
            &[
                0x06, 0x02, // Ty
                0x12, 0x00, // Len
                0x00, 0x00, 0x00, 0x0e, // Input Len
                0x05, 0x00, 0x00, 0x00, // Input Ty
                0x00, 0x00, // X
                0x00, 0x01, // Y
                0x00, 0x00, // Unused
                3, 232, // Reference Width
                3, 232, // Reference Height
            ],
        );
    }

    #[test]
    fn mouse_button() {
        test_packet(
            PacketDirection::ServerBound,
            sunshine_gen_7_config(),
            ControlPacket::MouseButton {
                action: MouseButtonAction::Press,
                button: MouseButton::Left,
            },
            &[
                0x06, 0x02, // Ty
                0x09, 0x00, // Len
                0x00, 0x00, 0x00, 0x05, // Input Len
                0x08, 0x00, 0x00, 0x00, // Mouse Action
                0x01, // Mouse Button
            ],
        );

        test_packet(
            PacketDirection::ServerBound,
            sunshine_gen_7_config(),
            ControlPacket::MouseButton {
                action: MouseButtonAction::Release,
                button: MouseButton::Left,
            },
            &[
                0x06, 0x02, // Ty
                0x09, 0x00, // Len
                0x00, 0x00, 0x00, 0x05, // Input Len
                0x09, 0x00, 0x00, 0x00, // Mouse Action
                0x01, // Mouse Button
            ],
        );
    }

    #[test]
    fn keyboard() {
        test_packet(
            PacketDirection::ServerBound,
            sunshine_gen_7_config(),
            ControlPacket::Keyboard {
                action: KeyAction::Down,
                flags: KeyFlags::empty(),
                key_code: KeyCode(0x41),
                modifier: KeyModifiers::CTRL,
                zero: 0,
            },
            &[
                0x06, 0x02, // Ty
                0x0e, 0x00, // Len
                0x00, 0x00, 0x00, 0x0a, // Input Len
                0x03, 0x00, 0x00, 0x00, // Key Action
                0x00, // Flags
                0x41, 0x00, // Key Code
                0x02, // Modifiers
                0x00, 0x00, // Zero
            ],
        );

        test_packet(
            PacketDirection::ServerBound,
            sunshine_gen_7_config(),
            ControlPacket::Keyboard {
                action: KeyAction::Up,
                flags: KeyFlags::SUNSHINE_NON_NORMALIZED,
                key_code: KeyCode(0x41),
                modifier: KeyModifiers::SHIFT,
                zero: 0,
            },
            &[
                0x06, 0x02, // Ty
                0x0e, 0x00, // Len
                0x00, 0x00, 0x00, 0x0a, // Input Len
                0x04, 0x00, 0x00, 0x00, // Key Action
                0x01, // Flags
                0x41, 0x00, // Key Code
                0x01, // Modifiers
                0x00, 0x00, // Zero
            ],
        );
    }
}
