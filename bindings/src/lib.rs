use std::net::SocketAddr;

use tracing_subscriber::util::TryInitError;
use uniffi::{
    Error, Record, custom_type,
    deps::anyhow::{Error, anyhow},
    remote, setup_scaffolding,
};

use moonlight_common::{
    ServerType, ServerVersion,
    stream::{
        AesIv, AesKey, SunshineEncryption,
        proto::{
            Instant, audio::AudioStreamError, control::peer::ControlError, packet::SunshinePing,
            video::VideoStreamError,
        },
        video::{VideoFormat, VideoFormats as VideoFormats2},
    },
    webrtc::WebRTCSessionParseError,
};

pub mod audio_stream;
pub mod control_packet;
pub mod control_serialization;
pub mod control_stream;
pub mod input_batcher;
pub mod video_stream;
pub mod webrtc;

pub mod log;

setup_scaffolding!();

#[derive(Debug, thiserror::Error, Error)]
#[uniffi(flat_error)]
pub enum MoonlightError {
    #[error("video stream: {0}")]
    VideoStream(#[from] VideoStreamError),
    #[error("audio stream: {0}")]
    AudioStream(#[from] AudioStreamError),
    #[error("control stream: {0}")]
    ControlStream(#[from] ControlError),
    #[error("webrtc session parse: {0}")]
    WebRTCSession(#[from] WebRTCSessionParseError),
    #[error("set logger: {0}")]
    Logger(#[from] TryInitError),
}

custom_type!(Instant, i64, {
    remote,
    // Lowering the Rust Instant into a u64.
    lower: |instant| instant.as_nanos(),
    // Lifting the foreign u64 into the Rust Instant
    try_lift: |nanos| Result::<_, Error>::Ok(Instant::from_nanos(nanos)),
});

custom_type!(SocketAddr, String, {
    remote,
    lower: |addr| addr.to_string(),
    try_lift: |text| Ok(text.parse()?),
});

custom_type!(AesKey, Vec<u8>, {
    remote,
    lower: |key| key.0.to_vec(),
    try_lift: |vec| Ok(AesKey(vec.as_array::<16>().copied().ok_or_else(|| anyhow!("The length of the AesKey must be 16 bytes! (current: {})", vec.len()))?)),
});

custom_type!(AesIv, u32, {
    remote,
    lower: |iv| iv.0,
    try_lift: |num| Ok(AesIv(num)),
});

#[remote(Record)]
pub struct SunshineEncryption {
    pub aes_key: AesKey,
    pub aes_iv: AesIv,
}

custom_type!(SunshinePing, Vec<u8>, {
    remote,
    lower: |ping| ping.0.to_vec(),
    try_lift: |vec| Ok(SunshinePing(vec.as_array::<16>().copied().ok_or_else(|| anyhow!("The length of the SunshinePing must be 16 bytes! (current: {})", vec.len()))?)),
});

#[remote(Record)]
pub struct ServerVersion {
    pub major: i32,
    pub minor: i32,
    pub patch: i32,
    pub sunshine_identifier: i32,
    pub server_type: ServerType,
}

#[remote(Enum)]
#[non_exhaustive]
pub enum ServerType {
    #[default]
    NvidiaGameStream,
    Sunshine,
    Apollo,
    FoundationSunshine,
}

#[remote(Enum)]
pub enum VideoFormat {
    H264,
    H264High8_444,
    H265,
    H265Main10,
    H265Rext8_444,
    H265Rext10_444,
    Av1Main8,
    Av1Main10,
    Av1High8_444,
    Av1High10_444,
}

#[derive(Debug, Record, Default, Clone, Copy, PartialEq, Eq)]
pub struct VideoFormats {
    pub h264: bool,
    pub h264_high8_444: bool,

    pub h265: bool,
    pub h265_main10: bool,
    pub h265_rext8_444: bool,
    pub h265_rext10_444: bool,

    pub av1_main8: bool,
    pub av1_main10: bool,
    pub av1_high8_444: bool,
    pub av1_high10_444: bool,
}

impl From<VideoFormats> for VideoFormats2 {
    fn from(value: VideoFormats) -> Self {
        let mut bits = Self::empty();

        if value.h264 {
            bits |= Self::H264;
        }

        if value.h264_high8_444 {
            bits |= Self::H264_HIGH8_444;
        }

        if value.h265 {
            bits |= Self::H265;
        }

        if value.h265_main10 {
            bits |= Self::H265_MAIN10;
        }

        if value.h265_rext8_444 {
            bits |= Self::H265_REXT8_444;
        }

        if value.h265_rext10_444 {
            bits |= Self::H265_REXT10_444;
        }

        if value.av1_main8 {
            bits |= Self::AV1_MAIN8;
        }

        if value.av1_main10 {
            bits |= Self::AV1_MAIN10;
        }

        if value.av1_high8_444 {
            bits |= Self::AV1_HIGH8_444;
        }

        if value.av1_high10_444 {
            bits |= Self::AV1_HIGH10_444;
        }

        bits
    }
}

impl From<VideoFormats2> for VideoFormats {
    fn from(value: VideoFormats2) -> Self {
        let mut bools = Self::default();

        if value.contains(VideoFormats2::H264) {
            bools.h264 = true;
        }

        if value.contains(VideoFormats2::H264_HIGH8_444) {
            bools.h264_high8_444 = true;
        }

        if value.contains(VideoFormats2::H265) {
            bools.h265 = true;
        }

        if value.contains(VideoFormats2::H265_MAIN10) {
            bools.h265_main10 = true;
        }

        if value.contains(VideoFormats2::H265_REXT8_444) {
            bools.h265_rext8_444 = true;
        }

        if value.contains(VideoFormats2::H265_REXT10_444) {
            bools.h265_rext10_444 = true;
        }

        if value.contains(VideoFormats2::AV1_MAIN8) {
            bools.av1_main8 = true;
        }

        if value.contains(VideoFormats2::AV1_MAIN10) {
            bools.av1_main10 = true;
        }

        if value.contains(VideoFormats2::AV1_HIGH8_444) {
            bools.av1_high8_444 = true;
        }

        if value.contains(VideoFormats2::AV1_HIGH10_444) {
            bools.av1_high10_444 = true;
        }

        bools
    }
}
