use std::time::Duration;

use log::trace;
use thiserror::Error;

use crate::ServerVersion;

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
    InputData,
    RumbleData,
    Termination,
    HdrMode,
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
    pub fn deserialize(ty: u16, server_version: ServerVersion, encrypted: bool) -> Option<Self> {
        match server_version.major {
            3 => match ty {
                0x0200 => Some(Self::PeriodicPing),
                0x1407 => Some(Self::RequestIdr),
                0x1410 => Some(Self::StartB),
                0x1404 => Some(Self::InvalidateReferenceFrames),
                0x140c => Some(Self::LossStats),
                0x1417 => Some(Self::FrameStats),
                _ => None,
            },
            4 => match ty {
                0x0200 => Some(Self::PeriodicPing),
                0x0606 => Some(Self::RequestIdr),
                0x0609 => Some(Self::StartB),
                0x0604 => Some(Self::InvalidateReferenceFrames),
                0x060a => Some(Self::LossStats),
                0x0611 => Some(Self::FrameStats),
                _ => None,
            },
            5 => match ty {
                0x0200 => Some(Self::PeriodicPing),
                0x0305 => Some(Self::RequestIdr),
                0x0307 => Some(Self::StartB),
                0x0301 => Some(Self::InvalidateReferenceFrames),
                0x0201 => Some(Self::LossStats),
                0x0204 => Some(Self::FrameStats),
                0x0207 => Some(Self::InputData),
                _ => None,
            },
            7 if encrypted => match ty {
                0x0200 if server_version >= PERIODIC_PING_VERSION => Some(Self::PeriodicPing),
                0x0302 => Some(Self::RequestIdr),
                0x0307 => Some(Self::StartB),
                0x0301 => Some(Self::InvalidateReferenceFrames),
                0x0201 => Some(Self::LossStats),
                0x0204 => Some(Self::FrameStats),
                0x0206 => Some(Self::InputData),
                0x010b => Some(Self::RumbleData),
                0x0109 => Some(Self::Termination),
                0x010e => Some(Self::HdrMode),
                // Sunshine protocol extensions
                0x5500 => Some(Self::RumbleTriggers),
                0x5501 => Some(Self::SetMotionEvent),
                0x5502 => Some(Self::SetRgbLed),
                0x5503 => Some(Self::SetAdaptiveTriggers),
                _ => None,
            },
            7 => match ty {
                0x0200 if server_version >= PERIODIC_PING_VERSION => Some(Self::PeriodicPing),
                0x0305 => Some(Self::RequestIdr),
                0x0307 => Some(Self::StartB),
                0x0301 => Some(Self::InvalidateReferenceFrames),
                0x0201 => Some(Self::LossStats),
                0x0204 => Some(Self::FrameStats),
                0x0206 => Some(Self::InputData),
                0x010b => Some(Self::RumbleData),
                0x0100 => Some(Self::Termination),
                0x010e => Some(Self::HdrMode),
                _ => None,
            },
            _ => None,
        }
    }
}

#[derive(Debug)]
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
}

impl ControlPacket {
    // TODO: what is the max size
    /// This is the maximum size a packet can have
    pub const SIZE: usize = 20;

    pub fn ty(&self) -> ControlPacketType {
        // TODO
        match self {
            Self::RequestIdr => ControlPacketType::RequestIdr,
            Self::StartB => ControlPacketType::StartB,
            _ => todo!(),
        }
    }

    /// Buffer is:
    /// - If not encrypted: the full payload
    /// - If encrypted: the decrypted payload -> it needs to be encrypted
    // TODO: make this return a result and handle error
    pub fn serialize(
        &self,
        server_version: ServerVersion,
        encrypted: bool,
        buffer: &mut [u8; Self::SIZE],
    ) -> usize {
        match self {
            Self::PeriodicPing => {
                let ty = ControlPacketType::PeriodicPing
                    .serialize(server_version, encrypted)
                    .unwrap();

                buffer[0..2].copy_from_slice(&ty.to_le_bytes());

                // TODO: is this correct? https://github.com/moonlight-stream/moonlight-common-c/blob/2a5a1f3e8a57cbbb316ed7dfff3a3965c2e77d25/src/ControlStream.c#L1395-L1396
                buffer[2..4].copy_from_slice(&4u16.to_le_bytes());
                buffer[4..8].copy_from_slice(&[0, 0, 0, 0]);

                8
            }
            Self::RequestIdr => {
                let ty = ControlPacketType::RequestIdr
                    .serialize(server_version, encrypted)
                    .unwrap();

                buffer[0..2].copy_from_slice(&ty.to_le_bytes());

                // https://github.com/moonlight-stream/moonlight-common-c/blob/435bc6a5a4852c90cfb037de1378c0334ed36d8e/src/ControlStream.c#L218-L227
                let contents = [0, 0];

                let len = 2 + contents.len();
                if buffer.len() < len {
                    // TODO: error?
                    todo!();
                }
                buffer[2..(contents.len() + 2)].copy_from_slice(&contents);

                len
            }
            Self::StartB => {
                let ty = ControlPacketType::StartB
                    .serialize(server_version, encrypted)
                    .unwrap();

                buffer[0..2].copy_from_slice(&ty.to_le_bytes());

                // https://github.com/moonlight-stream/moonlight-common-c/blob/435bc6a5a4852c90cfb037de1378c0334ed36d8e/src/ControlStream.c#L218-L227
                let contents: &[u8] = match server_version.major {
                    3 => &[0, 0, 0, 0xa],
                    _ => &[0],
                };

                let len = 2 + contents.len();
                if buffer.len() < len {
                    // TODO: error?
                    todo!();
                }
                buffer[2..(contents.len() + 2)].copy_from_slice(contents);

                len
            }
            _ => todo!(),
        }
        // TODO
    }

    // TODO: maybe replace option with an result?
    /// Payload is:
    /// - If not encrypted: the full payload
    /// - If encrypted: the decrypted payload
    pub fn deserialize(
        server_version: ServerVersion,
        encrypted: bool,
        payload: &[u8],
    ) -> Option<Self> {
        if payload.len() < 4 {
            return None;
        }
        let ty = u16::from_le_bytes([payload[0], payload[1]]);
        let len = u16::from_le_bytes([payload[2], payload[3]]);
        trace!(target: "moonlight_proto_control_packet", "Raw Ty: {ty:#x}, Len: {len}");

        // TODO
        let ty = ControlPacketType::deserialize(ty, server_version, encrypted)?;
        match ty {
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
            _ => todo!(),
        }
    }
}

// TODO: maybe more tests
#[cfg(test)]
mod test {
    #[test]
    fn test_control_packet_ty_serialize_deserialize() {
        // TODO: test that all ControlPacketType types serialize and deserialize to their correct types
        todo!()
    }
}
