use std::{ffi::CStr, fmt::Debug, time::Duration};

use bitflags::bitflags;
use moonlight_common_sys::limelight::{
    BUFFER_TYPE_PICDATA, BUFFER_TYPE_PPS, BUFFER_TYPE_SPS, BUFFER_TYPE_VPS,
    CAPABILITY_DIRECT_SUBMIT, CAPABILITY_PULL_RENDERER,
    CAPABILITY_REFERENCE_FRAME_INVALIDATION_AV1, CAPABILITY_REFERENCE_FRAME_INVALIDATION_AVC,
    CAPABILITY_REFERENCE_FRAME_INVALIDATION_HEVC, CAPABILITY_SLOW_OPUS_DECODER,
    CAPABILITY_SUPPORTS_ARBITRARY_AUDIO_DURATION, CONN_STATUS_OKAY, CONN_STATUS_POOR,
    FRAME_TYPE_IDR, FRAME_TYPE_PFRAME, LiGetStageName, ML_ERROR_FRAME_CONVERSION,
    ML_ERROR_GRACEFUL_TERMINATION, ML_ERROR_NO_VIDEO_FRAME, ML_ERROR_NO_VIDEO_TRAFFIC,
    ML_ERROR_PROTECTED_CONTENT, ML_ERROR_UNEXPECTED_EARLY_TERMINATION, STAGE_AUDIO_STREAM_INIT,
    STAGE_AUDIO_STREAM_START, STAGE_CONTROL_STREAM_INIT, STAGE_CONTROL_STREAM_START,
    STAGE_INPUT_STREAM_INIT, STAGE_INPUT_STREAM_START, STAGE_MAX, STAGE_NAME_RESOLUTION,
    STAGE_NONE, STAGE_PLATFORM_INIT, STAGE_RTSP_HANDSHAKE, STAGE_VIDEO_STREAM_INIT,
    STAGE_VIDEO_STREAM_START,
};
use num_derive::FromPrimitive;

use crate::stream::{
    ColorRange, ColorSpace, EncryptionFlags, StreamingConfig, SupportedVideoFormats,
};

// --------------- Stream ---------------

pub struct StreamConfiguration {
    /// Dimensions in pixels of the desired video stream
    pub width: i32,
    /// Dimensions in pixels of the desired video stream
    pub height: i32,
    /// FPS of the desired video stream
    pub fps: i32,
    /// Bitrate of the desired video stream (audio adds another ~1 Mbps). This
    /// includes error correction data, so the actual encoder bitrate will be
    /// about 20% lower when using the standard 20% FEC configuration.
    pub bitrate: i32,
    /// Max video packet size in bytes (use 1024 if unsure). If STREAM_CFG_AUTO
    /// determines the stream is remote (see below), it will cap this value at
    /// 1024 to avoid MTU-related issues like packet loss and fragmentation.
    pub packet_size: i32,
    /// Determines whether to enable remote (over the Internet)
    /// streaming optimizations. If unsure, set to STREAM_CFG_AUTO.
    /// STREAM_CFG_AUTO uses a heuristic (whether the target address is
    /// in the RFC 1918 address blocks) to decide whether the stream
    /// is remote or not.
    pub streaming_remotely: StreamingConfig,
    /// Specifies the channel configuration of the audio stream.
    /// See AUDIO_CONFIGURATION constants and MAKE_AUDIO_CONFIGURATION() below.
    pub audio_configuration: i32,
    /// Specifies the mask of supported video formats.
    /// See VIDEO_FORMAT constants below.
    pub supported_video_formats: SupportedVideoFormats,
    /// If specified, the client's display refresh rate x 100. For example,
    /// 59.94 Hz would be specified as 5994. This is used by recent versions
    /// of GFE for enhanced frame pacing.
    pub client_refresh_rate_x100: i32,
    /// If specified, sets the encoder colorspace to the provided COLORSPACE_*
    /// option (listed above). If not set, the encoder will default to Rec 601.
    pub color_space: ColorSpace,
    /// If specified, sets the encoder color range to the provided COLOR_RANGE_*
    /// option (listed above). If not set, the encoder will default to Limited.
    pub color_range: ColorRange,
    /// Specifies the data streams where encryption may be enabled if supported
    /// by the host PC. Ideally, you would pass ENCFLG_ALL to encrypt everything
    /// that we support encrypting. However, lower performance hardware may not
    /// be able to support encrypting heavy stuff like video or audio data, so
    /// that encryption may be disabled here. Remote input encryption is always
    /// enabled.
    pub encryption_flags: EncryptionFlags,
    /// AES encryption data for the remote input stream. This must be
    /// the same as what was passed as rikey and rikeyid
    /// in /launch and /resume requests.
    pub remote_input_aes_key: [u8; 16],
    /// AES encryption data for the remote input stream. This must be
    /// the same as what was passed as rikey and rikeyid
    /// in /launch and /resume requests.
    pub remote_input_aes_iv: u32,
}

// TODO: seperate into Audio and Video capabilities
bitflags! {
    #[derive(Debug, Clone, Copy, Default)]
    pub struct Capabilities: u32 {
        const DIRECT_SUBMIT = CAPABILITY_DIRECT_SUBMIT;
        const REFERENCE_FRAME_INVALIDATION_AV1 = CAPABILITY_REFERENCE_FRAME_INVALIDATION_AV1;
        const REFERENCE_FRAME_INVALIDATION_HEVC = CAPABILITY_REFERENCE_FRAME_INVALIDATION_HEVC;
        const REFERENCE_FRAME_INVALIDATION_AVC = CAPABILITY_REFERENCE_FRAME_INVALIDATION_AVC;
        const SUPPORTS_ARBITRARY_SOUND_DURATION = CAPABILITY_SUPPORTS_ARBITRARY_AUDIO_DURATION;
        const PULL_RENDERER = CAPABILITY_PULL_RENDERER;
        const SLOW_OPUS_DECODER = CAPABILITY_SLOW_OPUS_DECODER;
    }
}

#[repr(u32)]
#[derive(Debug, Clone, Copy, FromPrimitive)]
pub enum Stage {
    None = STAGE_NONE,
    PlatformInit = STAGE_PLATFORM_INIT,
    NameResolution = STAGE_NAME_RESOLUTION,
    AudioStreamInit = STAGE_AUDIO_STREAM_INIT,
    RtspHandshake = STAGE_RTSP_HANDSHAKE,
    ControlStreamInit = STAGE_CONTROL_STREAM_INIT,
    VideoStreamInit = STAGE_VIDEO_STREAM_INIT,
    InputStreamInit = STAGE_INPUT_STREAM_INIT,
    ControlStreamStart = STAGE_CONTROL_STREAM_START,
    VideoStreamStart = STAGE_VIDEO_STREAM_START,
    AudioStreamStart = STAGE_AUDIO_STREAM_START,
    InputStreamStart = STAGE_INPUT_STREAM_START,
    Max = STAGE_MAX,
}

impl Stage {
    pub fn name(&self) -> &str {
        unsafe {
            let raw_c_str = LiGetStageName(*self as i32);
            let c_str = CStr::from_ptr(raw_c_str);
            c_str.to_str().expect("convert stage name into utf8")
        }
    }
}

#[repr(u32)]
#[derive(Debug, Clone, Copy, FromPrimitive)]
pub enum ConnectionStatus {
    Ok = CONN_STATUS_OKAY,
    Poor = CONN_STATUS_POOR,
}

#[repr(i32)]
#[derive(Debug, Clone, Copy)]
pub enum TerminationError {
    Graceful = ML_ERROR_GRACEFUL_TERMINATION as i32,
    NoVideoTraffic = ML_ERROR_NO_VIDEO_TRAFFIC,
    NoVideoFrame = ML_ERROR_NO_VIDEO_FRAME,
    UnexpectedEarlyTermination = ML_ERROR_UNEXPECTED_EARLY_TERMINATION,
    ProtectedContent = ML_ERROR_PROTECTED_CONTENT,
    FrameConversion = ML_ERROR_FRAME_CONVERSION,
}

// --------------- Video ---------------
/// These identify codec configuration data in the buffer lists
/// of frames identified as IDR frames for H.264 and HEVC formats.
/// For other codecs, all data is marked as BUFFER_TYPE_PICDATA.
#[repr(u32)]
#[derive(Debug, Clone, Copy, FromPrimitive, PartialEq, Eq)]
pub enum BufferType {
    PicData = BUFFER_TYPE_PICDATA,
    Sps = BUFFER_TYPE_SPS,
    Pps = BUFFER_TYPE_PPS,
    Vps = BUFFER_TYPE_VPS,
}

#[repr(u32)]
#[derive(Debug, Clone, Copy, FromPrimitive)]
pub enum FrameType {
    /// This is a standard frame which references the IDR frame and
    /// previous P-frames.
    PFrame = FRAME_TYPE_PFRAME,
    /// This is a key frame.
    ///
    /// For H.264 and HEVC, this means the frame contains SPS, PPS, and VPS (HEVC only) NALUs
    /// as the first buffers in the list. The I-frame data follows immediately
    /// after the codec configuration NALUs.
    ///
    /// For other codecs, any configuration data is not split into separate buffers.
    Idr = FRAME_TYPE_IDR,
}

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Copy)]
pub struct EstimatedRttInfo {
    pub rtt: Duration,
    pub rtt_variance: Duration,
}
