use std::{
    fmt::{self, Display, Formatter},
    time::Duration,
};

use crate::stream::bindings::{
    BUFFER_TYPE_PICDATA, BUFFER_TYPE_PPS, BUFFER_TYPE_SPS, BUFFER_TYPE_VPS, COLOR_RANGE_FULL,
    COLOR_RANGE_LIMITED, COLORSPACE_REC_601, COLORSPACE_REC_709, COLORSPACE_REC_2020, DR_NEED_IDR,
    DR_OK, FRAME_TYPE_IDR, FRAME_TYPE_PFRAME, SCM_AV1_HIGH8_444, SCM_AV1_HIGH10_444, SCM_AV1_MAIN8,
    SCM_AV1_MAIN10, SCM_H264, SCM_H264_HIGH8_444, SCM_HEVC, SCM_HEVC_MAIN10, SCM_HEVC_REXT8_444,
    SCM_HEVC_REXT10_444, VIDEO_FORMAT_AV1_HIGH8_444, VIDEO_FORMAT_AV1_HIGH10_444,
    VIDEO_FORMAT_AV1_MAIN8, VIDEO_FORMAT_AV1_MAIN10, VIDEO_FORMAT_H264,
    VIDEO_FORMAT_H264_HIGH8_444, VIDEO_FORMAT_H265, VIDEO_FORMAT_H265_MAIN10,
    VIDEO_FORMAT_H265_REXT8_444, VIDEO_FORMAT_H265_REXT10_444, VIDEO_FORMAT_MASK_10BIT,
    VIDEO_FORMAT_MASK_AV1, VIDEO_FORMAT_MASK_H264, VIDEO_FORMAT_MASK_H265,
    VIDEO_FORMAT_MASK_YUV444,
};
use bitflags::bitflags;
use num_derive::FromPrimitive;

// https://github.com/moonlight-stream/moonlight-common-c/blob/3a377e7d7be7776d68a57828ae22283144285f90/src/RtspConnection.c#L1255
pub const DEFAULT_VIDEO_PORT: u16 = 47998;

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq)]
    pub struct ServerCodecModeSupport: u32 {
        const H264            = SCM_H264;
        const HEVC            = SCM_HEVC;
        const HEVC_MAIN10     = SCM_HEVC_MAIN10;
        const AV1_MAIN8       = SCM_AV1_MAIN8;
        const AV1_MAIN10      = SCM_AV1_MAIN10;
        const H264_HIGH8_444  = SCM_H264_HIGH8_444;
        const HEVC_REXT8_444  = SCM_HEVC_REXT8_444;
        const HEVC_REXT10_444 = SCM_HEVC_REXT10_444;
        const AV1_HIGH8_444   = SCM_AV1_HIGH8_444;
        const AV1_HIGH10_444  = SCM_AV1_HIGH10_444;
    }
}

#[repr(u32)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Copy, FromPrimitive)]
pub enum ColorSpace {
    Rec601 = COLORSPACE_REC_601,
    Rec709 = COLORSPACE_REC_709,
    Rec2020 = COLORSPACE_REC_2020,
}

#[repr(u32)]
#[derive(Debug, Clone, Copy, FromPrimitive)]
pub enum ColorRange {
    Limited = COLOR_RANGE_LIMITED,
    Full = COLOR_RANGE_FULL,
}

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Copy, Default)]
pub struct SupportedVideoFormats(u32);

bitflags! {
    impl SupportedVideoFormats: u32 {
        const H264 = VIDEO_FORMAT_H264;          // H.264 High Profile
        const H264_HIGH8_444 = VIDEO_FORMAT_H264_HIGH8_444;   // H.264 High 4:4:4 8-bit Profile
        const H265 = VIDEO_FORMAT_H265;                       // HEVC Main Profile
        const H265_MAIN10 = VIDEO_FORMAT_H265_MAIN10;         // HEVC Main10 Profile
        const H265_REXT8_444 = VIDEO_FORMAT_H265_REXT8_444;   // HEVC RExt 4:4:4 8-bit Profile
        const H265_REXT10_444 = VIDEO_FORMAT_H265_REXT10_444; // HEVC RExt 4:4:4 10-bit Profile
        const AV1_MAIN8 = VIDEO_FORMAT_AV1_MAIN8;             // AV1 Main 8-bit profile
        const AV1_MAIN10 = VIDEO_FORMAT_AV1_MAIN10;           // AV1 Main 10-bit profile
        const AV1_HIGH8_444 = VIDEO_FORMAT_AV1_HIGH8_444;     // AV1 High 4:4:4 8-bit profile
        const AV1_HIGH10_444 = VIDEO_FORMAT_AV1_HIGH10_444;   // AV1 High 4:4:4 10-bit profile

        // Preconfigured
        const MASK_H264 = VIDEO_FORMAT_MASK_H264;
        const MASK_H265 = VIDEO_FORMAT_MASK_H265;
        const MASK_AV1 = VIDEO_FORMAT_MASK_AV1;
        const MASK_10BIT = VIDEO_FORMAT_MASK_10BIT;
        const MASK_YUV444 = VIDEO_FORMAT_MASK_YUV444;
    }
}

impl Display for SupportedVideoFormats {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "[")?;

        let mut first = true;
        for (name, _) in self.iter_names() {
            if !first {
                write!(f, ",")?;
            }
            write!(f, "{}", name)?;

            first = false;
        }
        write!(f, "]")?;
        Ok(())
    }
}

#[repr(u32)]
#[derive(Debug, Clone, Copy, FromPrimitive)]
pub enum VideoFormat {
    H264 = VIDEO_FORMAT_H264,                      // H.264 High Profile
    H264High8_444 = VIDEO_FORMAT_H264_HIGH8_444,   // H.264 High 4:4:4 8-bit Profile
    H265 = VIDEO_FORMAT_H265,                      // HEVC Main Profile
    H265Main10 = VIDEO_FORMAT_H265_MAIN10,         // HEVC Main10 Profile
    H265Rext8_444 = VIDEO_FORMAT_H265_REXT8_444,   // HEVC RExt 4:4:4 8-bit Profile
    H265Rext10_444 = VIDEO_FORMAT_H265_REXT10_444, // HEVC RExt 4:4:4 10-bit Profile
    Av1Main8 = VIDEO_FORMAT_AV1_MAIN8,             // AV1 Main 8-bit profile
    Av1Main10 = VIDEO_FORMAT_AV1_MAIN10,           // AV1 Main 10-bit profile
    Av1High8_444 = VIDEO_FORMAT_AV1_HIGH8_444,     // AV1 High 4:4:4 8-bit profile
    Av1High10_444 = VIDEO_FORMAT_AV1_HIGH10_444,   // AV1 High 4:4:4 10-bit profile
}

impl VideoFormat {
    pub fn all() -> [Self; 10] {
        [
            VideoFormat::H264,
            VideoFormat::H264High8_444,
            VideoFormat::H265,
            VideoFormat::H265Main10,
            VideoFormat::H265Rext8_444,
            VideoFormat::H265Rext10_444,
            VideoFormat::Av1Main8,
            VideoFormat::Av1Main10,
            VideoFormat::Av1High8_444,
            VideoFormat::Av1High10_444,
        ]
    }

    pub fn contained_in(&self, supported_video_formats: SupportedVideoFormats) -> bool {
        let Some(single_format) = SupportedVideoFormats::from_bits(*self as u32) else {
            return false;
        };

        supported_video_formats.contains(single_format)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct VideoSetup {
    pub format: VideoFormat,
    pub width: u32,
    pub height: u32,
    pub redraw_rate: u32,
}

///
/// Sunshine Extension
///
/// References:
/// - https://github.com/moonlight-stream/moonlight-common-c/blob/62687809b1f7410c3db4be2527503a54ae408d70/src/Limelight.h#L967-L988
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SunshineHdrMetadata {
    pub display_primaries: [Primary; 3],
    pub white_point: Primary,
    pub max_display_luminance: u16,
    pub min_display_luminance: u16,
    pub max_content_light_level: u16,
    pub max_frame_average_light_level: u16,
    pub max_full_frame_luminance: u16,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Primary {
    pub x: u16,
    pub y: u16,
}

#[repr(i32)]
#[derive(Debug, Clone, Copy, Default)]
pub enum DecodeResult {
    #[default]
    Ok = DR_OK as i32,
    NeedIdr = DR_NEED_IDR,
}

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
#[derive(Debug, Clone, Copy, FromPrimitive, PartialEq)]
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

#[derive(Debug, PartialEq)]
pub struct VideoFrameBuffer<Buf> {
    /// The type of this buffer.
    /// See [BufferType] for more information.
    pub buffer_type: BufferType,
    pub data: Buf,
}

/// A decode unit describes a buffer chain of video data from multiple packets
// TODO: combine this type with the VideoFrame from the proto impl
pub struct VideoDecodeUnit<'a> {
    /// Frame Number
    pub frame_number: i32,

    pub frame_type: FrameType,

    /// Optional host processing latency of the frame, in 1/10 ms units.
    /// Zero when the host doesn't provide the latency data
    /// or frame processing latency is not applicable to the current frame
    /// (happens when the frame is repeated).
    pub frame_processing_latency: Option<Duration>,

    // TODO
    /// Receive time of first buffer. This value uses an implementation-defined epoch,
    /// but the same epoch as enqueueTimeMs and LiGetMillis().
    // pub receive_time: Duration,
    /// Time the frame was fully assembled and queued for the video decoder to process.
    // TODO
    /// This is also approximately the same time as the final packet was received, so
    /// enqueueTimeMs - receiveTimeMs is the time taken to receive the frame. At the
    /// time the decode unit is passed to submitDecodeUnit(), the total queue delay
    /// can be calculated by LiGetMillis() - enqueueTimeMs.
    // pub enqueue_time: Duration,

    /// The timestamp that the server sent.
    /// 90kHz clock time representation.
    ///
    /// References:
    /// - Moonlight common c: https://github.com/moonlight-stream/moonlight-common-c/blob/62687809b1f7410c3db4be2527503a54ae408d70/src/RtpVideoQueue.c#L157
    pub timestamp: Duration,
    /// Determines if this frame is SDR or HDR
    ///
    /// Note: This is not currently parsed from the actual bitstream, so if your
    /// client has access to a bitstream parser, prefer that over this field.
    pub hdr_active: bool,
    /// Provides the colorspace of this frame (see COLORSPACE_* defines above)
    ///
    /// Note: This is not currently parsed from the actual bitstream, so if your
    /// client has access to a bitstream parser, prefer that over this field.
    pub color_space: ColorSpace,
    pub buffers: &'a [VideoFrameBuffer<&'a [u8]>],
}

#[derive(Debug, Default)]
pub struct VideoCapabilities {
    pub reference_frame_invalidation_h264: bool,
    pub reference_frame_invalidation_h265: bool,
    pub reference_frame_invalidation_av1: bool,
    pub pull_renderer: bool,
}

// TODO: replace submit_decode_unit structs with more general structs
pub trait VideoDecoder {
    /// This callback is invoked to provide details about the video stream and allow configuration of the decoder.
    /// Returns 0 on success, non-zero on failure.
    fn setup(&mut self, setup: VideoSetup) -> i32;

    /// This callback notifies the decoder that the stream is starting. No frames can be submitted before this callback returns.
    fn start(&mut self);

    /// This callback provides Annex B formatted elementary stream data to the
    /// decoder. If the decoder is unable to process the submitted data for some reason,
    /// it must return DR_NEED_IDR to generate a keyframe.
    fn submit_decode_unit(&mut self, unit: VideoDecodeUnit<'_>) -> DecodeResult;

    /// This callback notifies the decoder that the stream is stopping. Frames may still be submitted but they may be safely discarded.
    fn stop(&mut self);

    fn supported_formats(&self) -> SupportedVideoFormats;
    fn capabilities(&self) -> VideoCapabilities {
        VideoCapabilities::default()
    }
}
