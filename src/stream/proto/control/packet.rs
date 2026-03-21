use std::time::Duration;

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
#[error(
    "packet type {packet:?} is not supported on server version {server_version} with encryption {encrypted}"
)]
pub struct ControlPacketNotSupported {
    packet: ControlPacketType,
    server_version: ServerVersion,
    encrypted: bool,
}

// TODO: maybe implement control over tcp for very old version
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

/// Encrypted Control Header:
/// Encryption requires version APP_VERSION_AT_LEAST(7, 1, 431):
/// - Version: https://github.com/moonlight-stream/moonlight-common-c/blob/435bc6a5a4852c90cfb037de1378c0334ed36d8e/src/ControlStream.c#L308
/// - Definition:
///   - https://games-on-whales.github.io/wolf/stable/protocols/control-specs.html#_encrypted_packet_format
///   - https://github.com/moonlight-stream/moonlight-common-c/blob/435bc6a5a4852c90cfb037de1378c0334ed36d8e/src/ControlStream.c#L25-L32
pub struct EncryptedControlHeader {
    /// The type of message, fixed at 0x0001 for this type of packet
    pub ty: u16,
    /// The size of the rest of the message in bytes (Seq + TAG + Payload)
    pub len: u16,
    /// Monotonically increasing sequence number (used as IV for AES-GCM)
    pub sequence_number: u16,
    /// The AES GCM TAG
    pub tag: [u8; 16],
}

impl EncryptedControlHeader {
    pub const SIZE: usize = 22;

    pub fn deserialize(buffer: &[u8; Self::SIZE]) -> Self {
        let ty = u16::from_be_bytes([buffer[0], buffer[1]]);
        let len = u16::from_be_bytes([buffer[2], buffer[3]]);
        let sequence_number = u16::from_be_bytes([buffer[4], buffer[5]]);

        // TODO: is the tag also little endian
        let mut tag = [0; 16];
        tag.copy_from_slice(&buffer[6..22]);

        Self {
            ty,
            len,
            sequence_number,
            tag,
        }
    }

    // TODO: error?
    pub fn serialize(&self, buffer: &mut [u8; Self::SIZE]) {
        if buffer.len() < 2 + 2 + 2 + 16 {
            todo!()
        }

        buffer[0..2].copy_from_slice(&self.ty.to_le_bytes());
        buffer[2..4].copy_from_slice(&self.len.to_le_bytes());
        buffer[4..6].copy_from_slice(&self.sequence_number.to_le_bytes());
        // TODO: is the tag also little endian?
        buffer[6..22].copy_from_slice(&self.tag);
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

// Packets:
// - New values: https://games-on-whales.github.io/wolf/stable/protocols/control-specs.html
// - Old Value: https://github.com/moonlight-stream/moonlight-common-c/blob/435bc6a5a4852c90cfb037de1378c0334ed36d8e/src/ControlStream.c#L146-L216
#[derive(Debug, Clone, Copy)]
pub enum ControlPacketType {
    /// See [ControlPacket::PeriodicPing]
    PeriodicPing,
    /// This seems to also equal StartA
    RequestIdr,
    StartB,
    InvalidateReferenceFrames,
    LossStats,
    FrameStats,
    RumbleData,
    Termination,
    HdrMode,
    /// An input packet.
    InputData,
    /// Sunshine Extension
    ///
    /// References:
    /// - Moonlight: https://github.com/moonlight-stream/moonlight-common-c/blob/62687809b1f7410c3db4be2527503a54ae408d70/src/Video.h#L57
    FrameFec,
    /// Sunshine Extension
    RumbleTriggers,
    /// Sunshine Extension
    SetMotionEvent,
    /// Sunshine Extension
    SetRgbLed,
    /// Sunshine Extension
    SetAdaptiveTriggers,
}

impl ControlPacketType {
    pub fn direction(&self) -> PacketDirection {
        match self {
            Self::PeriodicPing => PacketDirection::ServerBound,
            Self::RequestIdr => PacketDirection::ServerBound,
            Self::StartB => PacketDirection::ServerBound,
            Self::HdrMode => PacketDirection::ClientBound,
            Self::FrameFec => PacketDirection::ServerBound,
            Self::InputData => PacketDirection::ServerBound,
            _ => todo!(),
        }
    }

    pub fn serialize(
        &self,
        server_version: ServerVersion,
        encrypted: bool,
    ) -> Result<u16, ControlPacketNotSupported> {
        match server_version.major {
            3 => {
                // https://github.com/moonlight-stream/moonlight-common-c/blob/435bc6a5a4852c90cfb037de1378c0334ed36d8e/src/ControlStream.c#L146-L159
                match self {
                    Self::RequestIdr => Ok(0x1407),                // Request IDR frame
                    Self::StartB => Ok(0x1410),                    // Start B
                    Self::InvalidateReferenceFrames => Ok(0x1404), // Invalidate reference frames
                    Self::LossStats => Ok(0x140c),                 // Loss Stats
                    Self::FrameStats => Ok(0x1417),                // Frame Stats (unused)
                    Self::FrameFec if server_version.is_sunshine_like() => Ok(0x5502),
                    _ => Err(ControlPacketNotSupported {
                        packet: *self,
                        server_version,
                        encrypted,
                    }),
                }
            }
            4 => {
                // https://github.com/moonlight-stream/moonlight-common-c/blob/435bc6a5a4852c90cfb037de1378c0334ed36d8e/src/ControlStream.c#L160-L173
                match self {
                    Self::RequestIdr => Ok(0x0606),                // Request IDR frame
                    Self::StartB => Ok(0x0609),                    // Start B
                    Self::InvalidateReferenceFrames => Ok(0x0604), // Invalidate reference frames
                    Self::LossStats => Ok(0x060a),                 // Loss Stats
                    Self::FrameStats => Ok(0x0611),                // Frame Stats (unused)
                    Self::FrameFec if server_version.is_sunshine_like() => Ok(0x5502),
                    _ => Err(ControlPacketNotSupported {
                        packet: *self,
                        server_version,
                        encrypted,
                    }),
                }
            }
            5 => {
                // https://github.com/moonlight-stream/moonlight-common-c/blob/435bc6a5a4852c90cfb037de1378c0334ed36d8e/src/ControlStream.c#L175-L180
                match self {
                    Self::RequestIdr => Ok(0x0305),                // Start A
                    Self::StartB => Ok(0x0307),                    // Start B
                    Self::InvalidateReferenceFrames => Ok(0x0301), // Invalidate reference frames
                    Self::LossStats => Ok(0x0201),                 // Loss Stats
                    Self::FrameStats => Ok(0x0204),                // Frame Stats (unused)
                    Self::InputData => Ok(0x0207),                 // Input data
                    Self::FrameFec if server_version.is_sunshine_like() => Ok(0x5502),
                    _ => Err(ControlPacketNotSupported {
                        packet: *self,
                        server_version,
                        encrypted,
                    }),
                }
            }
            7 if encrypted => {
                // https://github.com/moonlight-stream/moonlight-common-c/blob/435bc6a5a4852c90cfb037de1378c0334ed36d8e/src/ControlStream.c#L202-L216
                match self {
                    Self::PeriodicPing if server_version >= PERIODIC_PING_VERSION => Ok(0x0200),
                    Self::RequestIdr => Ok(0x0302), // Request IDR frame
                    Self::StartB => Ok(0x0307),     // Start B
                    Self::InvalidateReferenceFrames => Ok(0x0301), // Invalidate reference frames
                    Self::LossStats => Ok(0x0201),  // Loss Stats
                    Self::FrameStats => Ok(0x0204), // Frame Stats (unused)
                    Self::InputData => Ok(0x0206),  // Input data
                    Self::RumbleData => Ok(0x010b), // Rumble data
                    Self::Termination => Ok(0x0109), // Termination (extended)
                    Self::HdrMode => Ok(0x010e),    // HDR mode
                    Self::RumbleTriggers => Ok(0x5500), // Rumble triggers (Sunshine protocol extension)
                    Self::SetMotionEvent => Ok(0x5501), // Set motion event (Sunshine protocol extension)
                    Self::SetRgbLed => Ok(0x5502),      // Set RGB LED (Sunshine protocol extension)
                    Self::SetAdaptiveTriggers => Ok(0x5503), // Set Adaptive Triggers (Sunshine protocol extension)
                    Self::FrameFec if server_version.is_sunshine_like() => Ok(0x5502),
                    _ => Err(ControlPacketNotSupported {
                        packet: *self,
                        server_version,
                        encrypted,
                    }),
                }
            }
            7 => {
                // https://github.com/moonlight-stream/moonlight-common-c/blob/435bc6a5a4852c90cfb037de1378c0334ed36d8e/src/ControlStream.c#L188-L201
                match self {
                    Self::PeriodicPing if server_version >= PERIODIC_PING_VERSION => Ok(0x0200),
                    Self::RequestIdr => Ok(0x0305), // Start A
                    Self::StartB => Ok(0x0307),     // Start B
                    Self::InvalidateReferenceFrames => Ok(0x0301), // Invalidate reference frames
                    Self::LossStats => Ok(0x0201),  // Loss Stats
                    Self::FrameStats => Ok(0x0204), // Frame Stats (unused)
                    Self::InputData => Ok(0x0206),  // Input data
                    Self::RumbleData => Ok(0x010b), // Rumble data
                    Self::Termination => Ok(0x0100), // Termination
                    Self::HdrMode => Ok(0x010e),    // HDR mode
                    Self::FrameFec if server_version.is_sunshine_like() => Ok(0x5502),
                    _ => Err(ControlPacketNotSupported {
                        packet: *self,
                        server_version,
                        encrypted,
                    }),
                }
            }
            _ => Err(ControlPacketNotSupported {
                packet: *self,
                server_version,
                encrypted,
            }),
        }
    }
    pub fn deserialize(
        ty: u16,
        direction: PacketDirection,
        server_version: ServerVersion,
        encrypted: bool,
    ) -> Option<Self> {
        match server_version.major {
            3 => match (direction, ty) {
                (PacketDirection::ServerBound, 0x0200) => Some(Self::PeriodicPing),
                (PacketDirection::ServerBound, 0x1407) => Some(Self::RequestIdr),
                (PacketDirection::ServerBound, 0x1410) => Some(Self::StartB),
                (PacketDirection::ServerBound, 0x1404) => Some(Self::InvalidateReferenceFrames),
                (PacketDirection::ServerBound, 0x140c) => Some(Self::LossStats),
                (PacketDirection::ServerBound, 0x1417) => Some(Self::FrameStats),
                (PacketDirection::ServerBound, 0x5502) if server_version.is_sunshine_like() => {
                    Some(Self::FrameFec)
                }
                _ => None,
            },
            4 => match (direction, ty) {
                (PacketDirection::ServerBound, 0x0200) => Some(Self::PeriodicPing),
                (PacketDirection::ServerBound, 0x0606) => Some(Self::RequestIdr),
                (PacketDirection::ServerBound, 0x0609) => Some(Self::StartB),
                (PacketDirection::ServerBound, 0x0604) => Some(Self::InvalidateReferenceFrames),
                (PacketDirection::ServerBound, 0x060a) => Some(Self::LossStats),
                (PacketDirection::ServerBound, 0x0611) => Some(Self::FrameStats),
                (PacketDirection::ServerBound, 0x5502) if server_version.is_sunshine_like() => {
                    Some(Self::FrameFec)
                }
                _ => None,
            },
            5 => match (direction, ty) {
                (PacketDirection::ServerBound, 0x0200) => Some(Self::PeriodicPing),
                (PacketDirection::ServerBound, 0x0305) => Some(Self::RequestIdr),
                (PacketDirection::ServerBound, 0x0307) => Some(Self::StartB),
                (PacketDirection::ServerBound, 0x0301) => Some(Self::InvalidateReferenceFrames),
                (PacketDirection::ServerBound, 0x0201) => Some(Self::LossStats),
                (PacketDirection::ServerBound, 0x0204) => Some(Self::FrameStats),
                (PacketDirection::ServerBound, 0x0207) => Some(Self::InputData),
                (PacketDirection::ServerBound, 0x5502) if server_version.is_sunshine_like() => {
                    Some(Self::FrameFec)
                }
                _ => None,
            },
            7 if encrypted => match (direction, ty) {
                (PacketDirection::ServerBound, 0x0200)
                    if server_version >= PERIODIC_PING_VERSION =>
                {
                    Some(Self::PeriodicPing)
                }
                (PacketDirection::ServerBound, 0x0302) => Some(Self::RequestIdr),
                (PacketDirection::ServerBound, 0x0307) => Some(Self::StartB),
                (PacketDirection::ServerBound, 0x0301) => Some(Self::InvalidateReferenceFrames),
                (PacketDirection::ServerBound, 0x0201) => Some(Self::LossStats),
                (PacketDirection::ServerBound, 0x0204) => Some(Self::FrameStats),
                (PacketDirection::ServerBound, 0x0206) => Some(Self::InputData),
                (PacketDirection::ClientBound, 0x010b) => Some(Self::RumbleData),
                (PacketDirection::ServerBound, 0x0109) => Some(Self::Termination),
                (PacketDirection::ClientBound, 0x010e) => Some(Self::HdrMode),
                // Sunshine protocol extensions
                (PacketDirection::ServerBound, 0x5502) => Some(Self::FrameFec),
                (PacketDirection::ClientBound, 0x5500) => Some(Self::RumbleTriggers),
                (PacketDirection::ServerBound, 0x5501) => Some(Self::SetMotionEvent),
                (PacketDirection::ClientBound, 0x5502) => Some(Self::SetRgbLed),
                (PacketDirection::ClientBound, 0x5503) => Some(Self::SetAdaptiveTriggers),
                _ => None,
            },
            7 => match (direction, ty) {
                (PacketDirection::ServerBound, 0x0200)
                    if server_version >= PERIODIC_PING_VERSION =>
                {
                    Some(Self::PeriodicPing)
                }
                (PacketDirection::ServerBound, 0x0305) => Some(Self::RequestIdr),
                (PacketDirection::ServerBound, 0x0307) => Some(Self::StartB),
                (PacketDirection::ServerBound, 0x0301) => Some(Self::InvalidateReferenceFrames),
                (PacketDirection::ServerBound, 0x0201) => Some(Self::LossStats),
                (PacketDirection::ServerBound, 0x0204) => Some(Self::FrameStats),
                (PacketDirection::ServerBound, 0x0206) => Some(Self::InputData),
                (PacketDirection::ClientBound, 0x010b) => Some(Self::RumbleData),
                (PacketDirection::ServerBound, 0x0100) => Some(Self::Termination),
                (PacketDirection::ClientBound, 0x010e) => Some(Self::HdrMode),
                (PacketDirection::ServerBound, 0x5502) => Some(Self::FrameFec),
                _ => None,
            },
            _ => None,
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
    /// Reports the video fec status to the server so it can adjust the amount of fec packets it sends.
    ///
    /// References:
    /// - moonlight sending: https://github.com/moonlight-stream/moonlight-common-c/blob/62687809b1f7410c3db4be2527503a54ae408d70/src/ControlStream.c#L1406-L1421
    /// - moonlight definition: https://github.com/moonlight-stream/moonlight-common-c/blob/62687809b1f7410c3db4be2527503a54ae408d70/src/Video.h#L56-L70
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
    // TODO: touch, controller, pen
}

impl ControlPacket {
    // TODO: what is the max size
    /// This is the maximum size a packet can have
    pub const MAX_SIZE: usize = 32;

    pub fn ty(&self) -> ControlPacketType {
        // TODO
        match self {
            Self::PeriodicPing => ControlPacketType::PeriodicPing,
            Self::RequestIdr => ControlPacketType::RequestIdr,
            Self::StartB => ControlPacketType::StartB,
            Self::HdrMode { .. } => ControlPacketType::HdrMode,
            Self::FrameFec { .. } => ControlPacketType::FrameFec,
            Self::MouseMoveRelative { .. } => ControlPacketType::InputData,
            Self::MouseMoveAbsolute { .. } => ControlPacketType::InputData,
            Self::MouseButton { .. } => ControlPacketType::InputData,
            Self::Keyboard { .. } => ControlPacketType::InputData,
            _ => todo!(),
        }
    }

    /// Buffer is:
    /// - If not encrypted: the full payload
    /// - If encrypted: the decrypted payload -> it needs to be encrypted
    // TODO: make this return a result and handle error
    #[instrument(level = Level::TRACE)]
    pub fn serialize(
        &self,
        server_version: ServerVersion,
        encrypted: bool,
        buffer: &mut [u8; Self::MAX_SIZE],
    ) -> Result<usize, ControlPacketNotSupported> {
        match self {
            Self::PeriodicPing => {
                let ty = ControlPacketType::PeriodicPing.serialize(server_version, encrypted)?;

                buffer[0..2].copy_from_slice(&ty.to_le_bytes());

                // Length of payload
                buffer[2..4].copy_from_slice(&4u16.to_le_bytes());

                // Timestamp?
                buffer[4..8].copy_from_slice(&[0, 0, 0, 0]);

                Ok(8)
            }
            Self::HdrMode { enabled, sunshine } => {
                // Ty
                let ty = ControlPacketType::HdrMode.serialize(server_version, encrypted)?;
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
                let ty = ControlPacketType::RequestIdr.serialize(server_version, encrypted)?;
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
                let ty = ControlPacketType::StartB.serialize(server_version, encrypted)?;
                buffer[0..2].copy_from_slice(&ty.to_le_bytes());

                // Length later

                // https://github.com/moonlight-stream/moonlight-common-c/blob/435bc6a5a4852c90cfb037de1378c0334ed36d8e/src/ControlStream.c#L218-L227
                let contents: &[u8] = match server_version.major {
                    3 => &[0, 0, 0, 0xa],
                    _ => &[0],
                };

                buffer[4..(contents.len() + 4)].copy_from_slice(contents);

                // Length
                buffer[2..4].copy_from_slice(&(contents.len() as u16).to_le_bytes());

                Ok(4 + contents.len())
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
                let ty = ControlPacketType::FrameFec.serialize(server_version, encrypted)?;
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
            Self::MouseMoveRelative { delta_x, delta_y } => {
                // Ty
                let ty = ControlPacketType::InputData.serialize(server_version, encrypted)?;
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
                let ty = ControlPacketType::InputData.serialize(server_version, encrypted)?;
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
                let ty = ControlPacketType::InputData.serialize(server_version, encrypted)?;
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
                let ty = ControlPacketType::InputData.serialize(server_version, encrypted)?;
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
        server_version: ServerVersion,
        encrypted: bool,
        payload: &[u8],
    ) -> Option<Self> {
        if payload.len() < 4 {
            warn!("Received packet that is too short (< 4 bytes)");
            return None;
        }
        let ty = u16::from_le_bytes([payload[0], payload[1]]);
        let len = u16::from_le_bytes([payload[2], payload[3]]);
        trace!("Raw Ty: {ty:#x}, Len: {len}");

        // TODO
        let ty = ControlPacketType::deserialize(ty, packet_direction, server_version, encrypted)?;
        trace!("Parsed Ty: {ty:?}");

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
                if server_version.is_sunshine_like() {
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
            proto::control::packet::{ControlPacket, PacketDirection},
            video::{Primary, SunshineHdrMetadata},
        },
    };

    fn test_packet(
        expected_packet_direction: PacketDirection,
        server_version: ServerVersion,
        encrypted: bool,
        expected_packet: ControlPacket,
        expected_bytes: &[u8],
    ) {
        let packet_direction = expected_packet.ty().direction();
        assert_eq!(
            packet_direction, expected_packet_direction,
            "Packet: {expected_packet:?}"
        );

        let mut bytes = [0; _];
        let len = expected_packet
            .serialize(server_version, encrypted, &mut bytes)
            .unwrap();
        let bytes = &bytes[0..len];
        assert_eq!(bytes, expected_bytes, "Serialize: {:?}", expected_packet);

        let packet =
            ControlPacket::deserialize(expected_packet_direction, server_version, encrypted, bytes)
                .unwrap();
        assert_eq!(
            packet, expected_packet,
            "Deserialize: {:?}",
            expected_packet
        );
    }

    const SUNSHINE_GEN_7: ServerVersion = ServerVersion::new(7, 1, 431, -1);

    #[test]
    fn ping() {
        test_packet(
            PacketDirection::ServerBound,
            SUNSHINE_GEN_7,
            false,
            ControlPacket::PeriodicPing,
            &[0, 2, 4, 0, 0, 0, 0, 0],
        );
    }

    #[test]
    fn hdr_mode() {
        init_test!();

        test_packet(
            PacketDirection::ClientBound,
            SUNSHINE_GEN_7,
            false,
            ControlPacket::HdrMode {
                enabled: false,
                sunshine: None,
            },
            &[14, 1, 1, 0, 0],
        );

        test_packet(
            PacketDirection::ClientBound,
            SUNSHINE_GEN_7,
            false,
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
            SUNSHINE_GEN_7,
            false,
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
            SUNSHINE_GEN_7,
            false,
            ControlPacket::RequestIdr,
            &[5, 3, 2, 0, 0, 0],
        );
    }

    #[test]
    fn start_b() {
        test_packet(
            PacketDirection::ServerBound,
            SUNSHINE_GEN_7,
            false,
            ControlPacket::StartB,
            &[7, 3, 1, 0, 0],
        );
    }

    #[test]
    fn frame_fec() {
        test_packet(
            PacketDirection::ServerBound,
            SUNSHINE_GEN_7,
            false,
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
    fn mouse_move_relative() {
        test_packet(
            PacketDirection::ServerBound,
            SUNSHINE_GEN_7,
            false,
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
            SUNSHINE_GEN_7,
            false,
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
            SUNSHINE_GEN_7,
            false,
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
            SUNSHINE_GEN_7,
            false,
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
            SUNSHINE_GEN_7,
            false,
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
            SUNSHINE_GEN_7,
            false,
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
