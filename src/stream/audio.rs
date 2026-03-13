use std::time::Duration;

use crate::stream::bindings::AUDIO_CONFIGURATION_MAX_CHANNEL_COUNT;
use thiserror::Error;

// https://github.com/moonlight-stream/moonlight-common-c/blob/3a377e7d7be7776d68a57828ae22283144285f90/src/RtspConnection.c#L1179
pub const DEFAULT_AUDIO_PORT: u16 = 48000;

/// This structure provides the Opus multistream decoder parameters required to successfully
/// decode the audio stream being sent from the computer. See opus_multistream_decoder_init docs
/// for details about these fields.
///
/// The supplied mapping array is indexed according to the following output channel order:
/// 0 - Front Left
/// 1 - Front Right
/// 2 - Center
/// 3 - LFE
/// 4 - Back Left
/// 5 - Back Right
/// 6 - Side Left
/// 7 - Side Right
///
/// If the mapping order does not match the channel order of the audio renderer, you may swap
/// the values in the mismatched indices until the mapping array matches the desired channel order.
#[derive(Debug, Clone)]
pub struct OpusMultistreamConfig {
    pub sample_rate: u32,
    pub channel_count: u32,
    pub streams: u32,
    pub coupled_streams: u32,
    pub samples_per_frame: u32,
    pub mapping: [u8; AUDIO_CONFIGURATION_MAX_CHANNEL_COUNT as usize],
}

impl OpusMultistreamConfig {
    pub const STEREO: OpusMultistreamConfig = OpusMultistreamConfig {
        sample_rate: 48000,
        channel_count: 2,
        streams: 1,
        coupled_streams: 1,
        samples_per_frame: 960,
        mapping: [0, 1, 0, 0, 0, 0, 0, 0],
    };

    pub fn frame_duration(&self) -> Duration {
        Duration::from_secs_f64(self.samples_per_frame as f64 / self.sample_rate as f64)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioConfig {
    pub channel_count: u32,
    pub channel_mask: u32,
}

#[derive(Debug, Error)]
#[error("failed to deserialize audio config!")]
pub struct FromRawAudioConfigError;

impl AudioConfig {
    /// Specifies that the audio stream should be encoded in stereo (default)
    pub const STEREO: AudioConfig = Self::new(2, 0x03);
    /// Specifies that the audio stream should be in 5.1 surround sound if the PC is able
    pub const SURROUND_51: AudioConfig = Self::new(6, 0x3F);
    /// Specifies that the audio stream should be in 7.1 surround sound if the PC is able
    pub const SURROUND_71: AudioConfig = Self::new(8, 0x63F);

    /// Specifies an audio configuration by channel count and channel mask
    /// See https://docs.microsoft.com/en-us/windows-hardware/drivers/audio/channel-mask for channelMask values
    /// NOTE: Not all combinations are supported by GFE and/or this library.
    pub const fn new(channel_count: u32, channel_mask: u32) -> Self {
        Self {
            channel_count,
            channel_mask,
        }
    }

    pub const fn from_surround_audio_info(info: u32) -> Self {
        Self {
            channel_count: info & 0xFFFF,
            channel_mask: info >> 16,
        }
    }

    pub const fn from_raw(raw: u32) -> Result<Self, FromRawAudioConfigError> {
        // Check the magic byte before decoding to make sure we got something that's actually
        // a MAKE_AUDIO_CONFIGURATION()-based value and not something else like an older version
        // hardcoded AUDIO_CONFIGURATION value from an earlier version of moonlight-common-c.
        if (raw & 0xFF) != 0xCA {
            return Err(FromRawAudioConfigError);
        }

        Ok(Self {
            channel_count: (raw >> 8) & 0xFF,
            channel_mask: (raw >> 16) & 0xFFFF,
        })
    }

    /// Helper function to retreive the surroundAudioInfo parameter value that must be passed in the /launch and /resume HTTPS requests when starting the session.
    ///
    /// References: https://github.com/moonlight-stream/moonlight-common-c/blob/62687809b1f7410c3db4be2527503a54ae408d70/src/Limelight.h#L215-L218
    pub fn to_surround_audio_info(&self) -> u32 {
        (self.channel_mask << 16) | self.channel_count
    }

    pub fn raw(&self) -> u32 {
        (self.channel_mask << 16) | (self.channel_count << 8) | 0xCA
    }
}

// TODO: make this use a lifetime instead of owning the vec
#[derive(Debug, PartialEq)]
pub struct AudioSample {
    /// Timestamps are in milliseconds
    ///
    /// When using moonlight common c timestamps are simulated because the library doesn't provide.
    /// This means that they could theoretically desync.
    ///
    /// References:
    /// - Sunshine https://github.com/LizardByte/Sunshine/blob/d157bb1d1eb7b0731cbf4caa7287bc7d715c5612/src/stream.cpp#L1646 and https://github.com/LizardByte/Sunshine/blob/master/src/rtsp.cpp#L971
    /// - Also see [crate::stream::proto::sdp::client::ClientSdp::audio_packet_duration]
    pub timestamp: Duration,
    pub buffer: Vec<u8>,
}

pub trait AudioDecoder {
    /// This callback initializes the audio renderer. The audio configuration parameter
    /// provides the negotiated audio configuration. This may differ from the one
    /// specified in the stream configuration. Returns 0 on success, non-zero on failure.
    fn setup(&mut self, audio_config: AudioConfig, stream_config: OpusMultistreamConfig) -> i32;

    /// This callback notifies the decoder that the stream is starting. No audio can be submitted before this callback returns.
    fn start(&mut self);

    /// This callback notifies the decoder that the stream is stopping. Audio samples may still be submitted but they may be safely discarded.
    fn stop(&mut self);

    /// This callback provides Opus audio data to be decoded and played. sampleLength is in bytes.
    fn decode_and_play_sample(&mut self, sample: AudioSample);

    fn config(&self) -> AudioConfig;
}
