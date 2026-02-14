use bitflags::bitflags;

/// The maximum amount of shards (data and parity together) that can be in one fec block.
///
/// References:
/// - https://github.com/games-on-whales/wolf/blob/1e375d318f5fbfbe88d27b4ca317ddb65841e9d0/src/moonlight-protocol/moonlight/fec.hpp#L20-L23
pub const MAX_VIDEO_SHARDS_PER_FEC_BLOCK: usize = 255;

/// The maximum amount of fec blocks per frame.
///
/// References:
/// - https://github.com/LizardByte/Sunshine/blob/69d7b6df27375c622db7e329f87dcd885efad76f/src/stream.cpp#L1351-L1352
pub const MAX_VIDEO_FEC_BLOCKS: usize = 4;

/// References:
/// - Moonlight: https://github.com/moonlight-stream/moonlight-common-c/blob/b126e481a195fdc7152d211def17190e3434bcce/src/Video.h#L12-L19
#[derive(Debug)]
pub struct EncryptedVideoHeader {
    pub iv: [u8; 12],
    /// Note: This value is little endian
    ///
    /// References:
    /// - Moonlight: https://github.com/moonlight-stream/moonlight-common-c/blob/435bc6a5a4852c90cfb037de1378c0334ed36d8e/src/VideoStream.c#L209
    pub frame_number: u32,
    pub tag: [u8; 16],
}

impl EncryptedVideoHeader {
    pub const SIZE: usize = 32;

    pub fn deserialize(buffer: &[u8; Self::SIZE]) -> Self {
        let mut iv = [0; _];
        iv.copy_from_slice(&buffer[0..12]);

        let frame_number = u32::from_le_bytes([buffer[12], buffer[13], buffer[14], buffer[15]]);

        let mut tag = [0; _];
        tag.copy_from_slice(&buffer[16..32]);

        Self {
            iv,
            frame_number,
            tag,
        }
    }
}

/// References:
/// - Moonlight: https://github.com/moonlight-stream/moonlight-common-c/blob/435bc6a5a4852c90cfb037de1378c0334ed36d8e/src/Video.h#L35
pub const VIDEO_FLAG_EXTENSION: u8 = 0x10;

/// References:
/// - https://games-on-whales.github.io/wolf/stable/protocols/rtp-video.html#_rtp_packets
#[derive(Debug, PartialEq)]
pub struct RtpVideoHeader {
    /// Must contain [FLAG_EXTENSION].
    /// Sunshine and Wolf also use 0x80 (0x80 | [FLAG_EXTENSION]).
    ///
    /// References:
    /// - Moonlight Assert: https://github.com/moonlight-stream/moonlight-common-c/blob/435bc6a5a4852c90cfb037de1378c0334ed36d8e/src/RtpVideoQueue.c#L549-L550
    /// - Sunshine: https://github.com/LizardByte/Sunshine/blob/69d7b6df27375c622db7e329f87dcd885efad76f/src/stream.cpp#L1486
    /// - Wolf: https://github.com/games-on-whales/wolf/blob/2c15d61107e48ca2fe3d350a703546aecb3eab78/src/moonlight-server/gst-plugin/video.hpp#L52
    pub header: u8,
    /// The packet type of this video packet.
    /// Usually zero.
    pub packet_type: u8,
    /// The sequence number of this packet.
    pub sequence_number: u16,
    /// The timestamp of this packet.
    pub timestamp: u32,
    /// Usually just zero.
    pub ssrc: u32,
    /// Reserved values.
    pub reserved: [u8; 4],
}

impl RtpVideoHeader {
    pub const SIZE: usize = 16;

    pub fn deserialize(buffer: &[u8; Self::SIZE]) -> Self {
        Self {
            header: u8::from_be_bytes([buffer[0]]),
            packet_type: u8::from_be_bytes([buffer[1]]),
            sequence_number: u16::from_be_bytes([buffer[2], buffer[3]]),
            timestamp: u32::from_be_bytes([buffer[4], buffer[5], buffer[6], buffer[7]]),
            ssrc: u32::from_be_bytes([buffer[8], buffer[9], buffer[10], buffer[11]]),
            reserved: [buffer[12], buffer[13], buffer[14], buffer[15]],
        }
    }

    pub fn serialize(&self, buffer: &mut [u8; Self::SIZE]) {
        buffer[0..1].copy_from_slice(&self.header.to_be_bytes());
        buffer[1..2].copy_from_slice(&self.packet_type.to_be_bytes());
        buffer[2..4].copy_from_slice(&self.sequence_number.to_be_bytes());
        buffer[4..8].copy_from_slice(&self.timestamp.to_be_bytes());
        buffer[8..12].copy_from_slice(&self.ssrc.to_be_bytes());
        buffer[12..16].copy_from_slice(&self.reserved);
    }
}

bitflags! {
    /// Flags for each packet.
    ///
    /// References:
    /// - https://games-on-whales.github.io/wolf/stable/protocols/rtp-video.html#_nv_video_flags
    /// - Moonlight: https://github.com/moonlight-stream/moonlight-common-c/blob/435bc6a5a4852c90cfb037de1378c0334ed36d8e/src/Video.h#L21-L23
    /// - Sunshine construction: https://github.com/LizardByte/Sunshine/blob/69d7b6df27375c622db7e329f87dcd885efad76f/src/stream.cpp#L1437-L1442
    #[derive(Debug, Clone, Copy, PartialEq)]
    pub struct VideoHeaderFlags: u8 {
        const CONTAINS_VIDEO_DATA = 0x01;
        const END_OF_FILE = 0x02;
        const START_OF_FILE = 0x04;
    }
}

impl VideoHeaderFlags {
    pub fn from_index(index: usize, total_size: usize, contains_video_data: bool) -> Self {
        let mut flags = VideoHeaderFlags::empty();

        if contains_video_data {
            flags |= VideoHeaderFlags::CONTAINS_VIDEO_DATA;
        }
        if index == 0 {
            flags |= VideoHeaderFlags::START_OF_FILE;
        }
        if index == total_size.saturating_sub(1) {
            flags |= VideoHeaderFlags::END_OF_FILE;
        }

        flags
    }
}

/// References:
/// - https://games-on-whales.github.io/wolf/stable/protocols/rtp-video.html#_rtp_packets
/// - Wolf: https://github.com/games-on-whales/wolf/blob/2c15d61107e48ca2fe3d350a703546aecb3eab78/src/moonlight-protocol/moonlight/data-structures.hpp#L41-L49
#[derive(Debug, PartialEq)]
pub struct VideoHeader {
    /// This seems to just be `(sequence_number) << 8`
    ///
    /// Note: This value is little endian
    ///
    /// References:
    /// - Sunshine: https://github.com/LizardByte/Sunshine/blob/69d7b6df27375c622db7e329f87dcd885efad76f/src/stream.cpp#L1431
    /// - Moonlight: https://github.com/moonlight-stream/moonlight-common-c/blob/435bc6a5a4852c90cfb037de1378c0334ed36d8e/src/RtpVideoQueue.c#L564
    pub stream_packet_index: u32,
    /// The index of the current frame.
    ///
    /// Note: This value is little endian
    ///
    /// References:
    /// - Sunshine: https://github.com/LizardByte/Sunshine/blob/69d7b6df27375c622db7e329f87dcd885efad76f/src/stream.cpp#L1430
    /// - Moonlight: https://github.com/moonlight-stream/moonlight-common-c/blob/435bc6a5a4852c90cfb037de1378c0334ed36d8e/src/RtpVideoQueue.c#L565
    pub frame_index: u32,
    /// See the struct for more info
    pub flags: VideoHeaderFlags,
    // TODO: The LTR PR adds this as extraFlags: https://github.com/moonlight-stream/moonlight-common-c/blob/2a5a1f3e8a57cbbb316ed7dfff3a3965c2e77d25/src/Video.h#L25
    // https://github.com/moonlight-stream/moonlight-common-c/blob/305993b01322aeb7710a5443960774ecd391c55c/src/Video.h#L31
    /// Usually just zero
    pub reserved: u8,
    /// Should contain 0x10
    ///
    /// References:
    /// - Sunshine: https://github.com/LizardByte/Sunshine/blob/69d7b6df27375c622db7e329f87dcd885efad76f/src/stream.cpp#L1433-L1434
    /// - Wolf: https://github.com/games-on-whales/wolf/blob/2c15d61107e48ca2fe3d350a703546aecb3eab78/src/moonlight-server/gst-plugin/video.hpp#L154
    pub multi_fec_flags: u8,
    /// See struct for more info
    pub multi_fec_blocks: VideoMultiFecBlocks,
    /// See struct for more info
    ///
    /// Note: This value is little endian
    pub fec_info: VideoFecInfo,
}

impl VideoHeader {
    pub const SIZE: usize = 16;

    pub fn deserialize(buffer: &[u8; Self::SIZE]) -> VideoHeader {
        let stream_packet_index = u32::from_le_bytes([buffer[0], buffer[1], buffer[2], buffer[3]]);
        let frame_index = u32::from_le_bytes([buffer[4], buffer[5], buffer[6], buffer[7]]);
        let flags = VideoHeaderFlags::from_bits_retain(buffer[8]);
        let reserved = buffer[9];
        let multi_fec_flags = buffer[10];
        let multi_fec_blocks = VideoMultiFecBlocks::deserialize(buffer[11]);
        let fec_info = VideoFecInfo::deserialize(u32::from_le_bytes([
            buffer[12], buffer[13], buffer[14], buffer[15],
        ]));

        VideoHeader {
            stream_packet_index,
            frame_index,
            flags,
            reserved,
            multi_fec_flags,
            multi_fec_blocks,
            fec_info,
        }
    }

    pub fn serialize(&self, buffer: &mut [u8; Self::SIZE]) {
        buffer[0..4].copy_from_slice(&self.stream_packet_index.to_le_bytes());
        buffer[4..8].copy_from_slice(&self.frame_index.to_le_bytes());
        buffer[8] = self.flags.bits();
        buffer[9] = self.reserved;
        buffer[10] = self.multi_fec_flags;
        buffer[11] = self.multi_fec_blocks.serialize();
        buffer[12..16].copy_from_slice(&self.fec_info.serialize().to_le_bytes());
    }
}

/// TODO: this always seems to be zero in the video queue, why?
///
/// References:
/// - Sunshine: https://github.com/LizardByte/Sunshine/blob/69d7b6df27375c622db7e329f87dcd885efad76f/src/stream.cpp#L1435
/// - Wolf: https://github.com/games-on-whales/wolf/blob/2c15d61107e48ca2fe3d350a703546aecb3eab78/src/moonlight-server/gst-plugin/video.hpp#L153
/// - Moonlight: https://github.com/moonlight-stream/moonlight-common-c/blob/b126e481a195fdc7152d211def17190e3434bcce/src/RtpVideoQueue.c#L584
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VideoMultiFecBlocks {
    /// Bits 6..8 (exclusive), 2 Bits
    pub block_index: u8,
    /// Bits 4..6 (exclusive), 2 Bits
    pub current_block: u8,
    /// Bits 0..4 (exclusive), 4 Bits
    pub unused: u8,
}

impl VideoMultiFecBlocks {
    pub fn deserialize(value: u8) -> Self {
        let block_index = (value >> 6) & 0b11;
        let current_block = (value >> 4) & 0b11;
        let unused = value & 0b1111;

        Self {
            block_index,
            current_block,
            unused,
        }
    }

    pub fn serialize(&self) -> u8 {
        debug_assert_eq!(self.block_index & !0b11, 0);
        debug_assert_eq!(self.current_block & !0b11, 0);
        debug_assert_eq!(self.unused & !0b1111, 0);

        ((self.block_index & 0b11) << 6)
            | ((self.current_block & 0b11) << 4)
            | (self.unused & 0b1111)
    }
}

/// Note: This value is little endian
///
/// References:
/// - Moonlight Parse: https://github.com/moonlight-stream/moonlight-common-c/blob/435bc6a5a4852c90cfb037de1378c0334ed36d8e/src/RtpVideoQueue.c#L566
/// - Moonlight Deconstruct: https://github.com/moonlight-stream/moonlight-common-c/blob/b126e481a195fdc7152d211def17190e3434bcce/src/RtpVideoQueue.c#L583
/// - Moonlight Deconstruct: https://github.com/moonlight-stream/moonlight-common-c/blob/b126e481a195fdc7152d211def17190e3434bcce/src/RtpVideoQueue.c#L703-L704
/// - Sunshine: https://github.com/LizardByte/Sunshine/blob/69d7b6df27375c622db7e329f87dcd885efad76f/src/stream.cpp#L1481-L1484
/// - Wolf: https://github.com/games-on-whales/wolf/blob/2c15d61107e48ca2fe3d350a703546aecb3eab78/src/moonlight-server/gst-plugin/video.hpp#L152
#[derive(Debug, PartialEq)]
pub struct VideoFecInfo {
    /// Bits 22..32 (exclusive), 10 Bits
    ///
    /// The amount of data shards in this frame (see [VideoHeader::frame_index]).
    pub data_shards_total: u32,
    /// Bits 12..22 (exclusive), 10 Bits
    ///
    /// The shard index of this data in the frame (see [VideoHeader::frame_index]).
    pub shard_index: u32,
    /// Bits 4..12 (exclusive), 8 Bits
    ///
    /// The fec percentage of this frame (see [VideoHeader::frame_index]).
    pub fec_percentage: u32,
    /// Bits 0..4 (exclusive), 4 Bits
    ///
    /// Usually just zero.
    pub unused: u32,
}

impl VideoFecInfo {
    pub fn deserialize(value: u32) -> Self {
        let data_shards_total = (value >> 22) & 0b11_1111_1111;
        let shard_index = (value >> 12) & 0b11_1111_1111;
        let fec_percentage = (value >> 4) & 0b1111_1111;
        let unused = value & 0b1111;

        Self {
            data_shards_total,
            shard_index,
            fec_percentage,
            unused,
        }
    }

    pub fn serialize(&self) -> u32 {
        debug_assert_eq!(self.data_shards_total & !0b11_1111_1111, 0); // 10 bits
        debug_assert_eq!(self.shard_index & !0b11_1111_1111, 0); // 10 bits
        debug_assert_eq!(self.fec_percentage & !0b1111_1111, 0); // 8 bits
        debug_assert_eq!(self.unused & !0b1111, 0); // 4 bits

        ((self.data_shards_total & 0b11_1111_1111) << 22)
            | ((self.shard_index & 0b11_1111_1111) << 12)
            | ((self.fec_percentage & 0b1111_1111) << 4)
            | (self.unused & 0b1111)
    }
}

/// Returns the fec shards based on data shards and fec percentage
///
/// References:
/// - https://github.com/moonlight-stream/moonlight-common-c/blob/2a5a1f3e8a57cbbb316ed7dfff3a3965c2e77d25/src/RtpVideoQueue.c#L705
pub fn fec_percentage_to_parity_shards(data_shards: usize, fec_percentage: usize) -> usize {
    (data_shards * fec_percentage).div_ceil(100)
}

/// Returns the fec shards based on data shards and fec percentage
///
/// References:
/// - https://github.com/games-on-whales/wolf/blob/2c15d61107e48ca2fe3d350a703546aecb3eab78/src/moonlight-server/gst-plugin/video.hpp#L173-L177
pub fn fec_percentage_from(data_shards: usize, parity_shards: usize) -> usize {
    (100 * parity_shards) / data_shards
}

/// The header of a single frame that was be reconstructed from multiples packets.
///
/// References:
/// - https://github.com/games-on-whales/wolf/blob/2c15d61107e48ca2fe3d350a703546aecb3eab78/src/moonlight-server/gst-plugin/video.hpp#L19-L34
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VideoFrameHeader {
    /// Always 0x01 for short headers
    ///
    /// References:
    /// - https://github.com/games-on-whales/wolf/blob/2c15d61107e48ca2fe3d350a703546aecb3eab78/src/moonlight-server/gst-plugin/video.hpp#L20C24-L20C56
    pub header_type: u8,
    pub unknown: [u8; 2],
    /// See [FrameType]
    pub frame_type: FrameType,
    /// Length of the final packet payload for codecs that cannot handle
    /// zero padding, such as AV1 (Sunshine extension).
    ///
    /// Note: little endian
    ///
    /// References:
    /// - https://github.com/games-on-whales/wolf/blob/2c15d61107e48ca2fe3d350a703546aecb3eab78/src/moonlight-server/gst-plugin/video.hpp#L31
    pub last_payload_len: u16,
    pub unknown2: [u8; 2],
}

impl VideoFrameHeader {
    pub const SIZE: usize = 8;

    pub fn deserialize(buffer: &[u8; Self::SIZE]) -> VideoFrameHeader {
        let header_type = buffer[0];
        let unknown = [buffer[1], buffer[2]];
        let frame_type = FrameType::deserialize(buffer[3]);
        let last_payload_len = u16::from_le_bytes([buffer[4], buffer[5]]);
        let unknown2 = [buffer[6], buffer[7]];

        VideoFrameHeader {
            header_type,
            unknown,
            frame_type,
            last_payload_len,
            unknown2,
        }
    }

    pub fn serialize(&self, buffer: &mut [u8; Self::SIZE]) {
        buffer[0] = self.header_type;
        buffer[1..3].copy_from_slice(&self.unknown);
        buffer[3] = self.frame_type.serialize();
        buffer[4..6].copy_from_slice(&self.last_payload_len.to_le_bytes());
        buffer[6..8].copy_from_slice(&self.unknown2);
    }
}

/// Currently known values:
/// - 1 = Normal P-frame
/// - 2 = IDR-frame
/// - 4 = P-frame with intra-refresh blocks
/// - 5 = P-frame after reference frame invalidation
///
/// References:
/// - https://github.com/games-on-whales/wolf/blob/2c15d61107e48ca2fe3d350a703546aecb3eab78/src/moonlight-server/gst-plugin/video.hpp#L22-L27
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FrameType {
    /// Normal P-frame
    ///
    /// Note: 1
    PFrame,
    /// IDR-frame
    ///
    /// Note: 2
    Idr,
    /// P-frame with intra-refresh blocks
    ///
    /// Note: 4
    PFrameIntra,
    /// P-frame after reference frame invalidation
    ///
    /// Note: 5
    PFrameReferenceInvalidation,
    #[doc(hidden)]
    Other(u8),
}

impl FrameType {
    pub fn serialize(&self) -> u8 {
        match self {
            FrameType::PFrame => 1,
            FrameType::Idr => 2,
            FrameType::PFrameIntra => 4,
            FrameType::PFrameReferenceInvalidation => 5,
            FrameType::Other(val) => *val, // Return the value of the `Other` variant
        }
    }

    pub fn deserialize(value: u8) -> FrameType {
        match value {
            1 => FrameType::PFrame,
            2 => FrameType::Idr,
            4 => FrameType::PFrameIntra,
            5 => FrameType::PFrameReferenceInvalidation,
            other => FrameType::Other(other),
        }
    }
}
