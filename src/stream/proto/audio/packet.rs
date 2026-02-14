// https://github.com/moonlight-stream/moonlight-common-c/blob/3a377e7d7be7776d68a57828ae22283144285f90/src/AudioStream.c#L22
pub const INVALID_OPUS_HEADER: u8 = 0;

/// This is the header used for audio.
/// There are no checks that disallow this header from being another value.
///
/// References:
/// - Sunshine: https://github.com/LizardByte/Sunshine/blob/69d7b6df27375c622db7e329f87dcd885efad76f/src/stream.cpp#L1608
/// - Wolf: https://github.com/games-on-whales/wolf/blob/2c15d61107e48ca2fe3d350a703546aecb3eab78/src/moonlight-server/gst-plugin/audio.hpp#L43
pub const RTP_AUDIO_HEADER: u8 = 0x80;

// https://github.com/moonlight-stream/moonlight-common-c/blob/3a377e7d7be7776d68a57828ae22283144285f90/src/RtpAudioQueue.c#L18-L19
pub const RTP_PAYLOAD_TYPE_AUDIO: u8 = 97;
pub const RTP_PAYLOAD_TYPE_AUDIO_FEC: u8 = 127;

/// References:
/// - https://games-on-whales.github.io/wolf/stable/protocols/rtp-opus.html#_rtp_packets
#[derive(Debug, PartialEq)]
pub struct RtpAudioHeader {
    /// Seems to just be [RTP_HEADER_AUDIO].
    ///
    /// See [RTP_HEADER_AUDIO] for more info
    pub header: u8,
    /// Either [RTP_PAYLOAD_TYPE_AUDIO] or [RTP_PAYLOAD_TYPE_FEC]
    ///
    /// References:
    /// - Moonlight: https://github.com/moonlight-stream/moonlight-common-c/blob/435bc6a5a4852c90cfb037de1378c0334ed36d8e/src/RtpAudioQueue.c#L18-L19
    pub packet_type: u8,
    /// The sequence number of the packet.
    pub sequence_number: u16,
    /// The timestamp of the sample.
    pub timestamp: u32,
    /// Usually just zero.
    pub ssrc: u32,
}

impl RtpAudioHeader {
    pub const SIZE: usize = 12;

    pub fn deserialize(buffer: &[u8; Self::SIZE]) -> Self {
        Self {
            header: u8::from_be_bytes([buffer[0]]),
            packet_type: u8::from_be_bytes([buffer[1]]),
            sequence_number: u16::from_be_bytes([buffer[2], buffer[3]]),
            timestamp: u32::from_be_bytes([buffer[4], buffer[5], buffer[6], buffer[7]]),
            ssrc: u32::from_be_bytes([buffer[8], buffer[9], buffer[10], buffer[11]]),
        }
    }

    pub fn serialize(&self, buffer: &mut [u8; Self::SIZE]) {
        buffer[0..1].copy_from_slice(&self.header.to_be_bytes());
        buffer[1..2].copy_from_slice(&self.packet_type.to_be_bytes());
        buffer[2..4].copy_from_slice(&self.sequence_number.to_be_bytes());
        buffer[4..8].copy_from_slice(&self.timestamp.to_be_bytes());
        buffer[8..12].copy_from_slice(&self.ssrc.to_be_bytes());
    }
}

pub const RTP_AUDIO_DATA_SHARDS: usize = 4;
pub const RTP_AUDIO_FEC_SHARDS: usize = 2;
pub const RTP_AUDIO_TOTAL_SHARDS: usize = RTP_AUDIO_DATA_SHARDS + RTP_AUDIO_FEC_SHARDS;

/// An audio fec header.
/// Exists after the normal RtpAudioHeader when [RtpAudioHeader::packet_type] == [RTP_PAYLOAD_TYPE_FEC].
///
/// Sunshine normally sends 4 data packets and 2 fec packets.
/// Those two fec packets have the [AudioFecHeader::fec_shard_index] 0 and 1 and can be used for fec reconstruction.
///
/// References:
/// - https://games-on-whales.github.io/wolf/stable/protocols/rtp-opus.html#_rtp_packets
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AudioFecHeader {
    /// The shard index for fec reconstruction.
    pub fec_shard_index: u8,
    /// The payload type of this fec packet.
    ///
    /// Usually just 97 = [RTP_PAYLOAD_TYPE_FEC].
    pub payload_type: u8,
    /// The base sequence number this fec packet refers to.
    pub base_sequence_number: u16,
    /// The base timestamp of this fec block.
    pub base_timestamp: u32,
    /// Usually just zero.
    pub ssrc: u32,
}

impl AudioFecHeader {
    pub const SIZE: usize = 12;

    pub fn deserialize(buffer: &[u8; Self::SIZE]) -> Self {
        let fec_shard_index = u8::from_be_bytes([buffer[0]]);
        let payload_type = u8::from_be_bytes([buffer[1]]);
        let base_sequence_number = u16::from_be_bytes([buffer[2], buffer[3]]);
        let base_timestamp = u32::from_be_bytes([buffer[4], buffer[5], buffer[6], buffer[7]]);
        let ssrc = u32::from_be_bytes([buffer[8], buffer[9], buffer[10], buffer[11]]);

        Self {
            fec_shard_index,
            payload_type,
            base_sequence_number,
            base_timestamp,
            ssrc,
        }
    }

    pub fn serialize(&self, buffer: &mut [u8; Self::SIZE]) {
        buffer[0..1].copy_from_slice(&self.fec_shard_index.to_be_bytes());
        buffer[1..2].copy_from_slice(&self.payload_type.to_be_bytes());
        buffer[2..4].copy_from_slice(&self.base_sequence_number.to_be_bytes());
        buffer[4..8].copy_from_slice(&self.base_timestamp.to_be_bytes());
        buffer[8..12].copy_from_slice(&self.ssrc.to_be_bytes());
    }
}
