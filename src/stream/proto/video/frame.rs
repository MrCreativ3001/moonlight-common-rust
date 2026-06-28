use std::time::Duration;

use bytes::Bytes;
use smallvec::smallvec;

use crate::stream::{
    proto::video::{
        nal::{ParsedNalus, parse_nalus},
        packet::FrameType,
    },
    video::{
        self, BufferType, ColorSpace, FrameIndex, VideoDecodeUnit, VideoDecodeUnitBuffers,
        VideoFormat, VideoFormats, VideoFrameBuffer,
    },
};

#[derive(Debug, Clone, PartialEq)]
pub struct VideoFrameMetadata {
    /// The index of the frame
    pub frame_index: FrameIndex,
    /// Type of this frame.
    pub frame_type: FrameType,
    /// The timestamp that the server sent.
    /// 90kHz clock time representation.
    ///
    /// References:
    /// - Moonlight common c: <https://github.com/moonlight-stream/moonlight-common-c/blob/62687809b1f7410c3db4be2527503a54ae408d70/src/RtpVideoQueue.c#L157>
    pub timestamp: Duration,
    /// The processing latency of the host
    ///
    /// References:
    /// - <https://github.com/moonlight-stream/moonlight-common-c/blob/7b026e77be62175104640e7e722b758df6d3d0d7/src/Limelight.h#L151-L155>
    pub host_processing_latency: Option<Duration>,
}

#[derive(Debug, PartialEq)]
pub struct VideoFrame<'a> {
    /// Metadata of a video frame.
    pub metadata: VideoFrameMetadata,
    /// Parsed type of this frame.
    ///
    /// The difference to [Self::frame_type] is that this is directly parsed from the bitstream for some codecs.
    /// - For H264 and H265 this will be parsed using the nalus from the bitstream.
    /// - For other codecs (Av1) this will be the value from the server
    pub parsed_frame_type: video::FrameType,
    /// The buffers this frame consists of.
    ///
    /// Different codecs split buffers differently:
    /// - H264: each buffer starts with an annex b start code followed by a h264 nalu.
    /// - H265: each buffer starts with an annex b start code followed by a h265 nalu.
    /// - Av1: no specific point where they're being split
    pub buffers: VideoDecodeUnitBuffers<&'a [u8]>,
}

impl<'a> VideoFrame<'a> {
    pub fn into_decode_unit(self) -> VideoDecodeUnit<&'a [u8]> {
        VideoDecodeUnit {
            frame_number: self.metadata.frame_index,
            frame_type: self.parsed_frame_type,
            frame_processing_latency: self.metadata.host_processing_latency,
            timestamp: self.metadata.timestamp,
            // TODO: how to get this?
            color_space: ColorSpace::Rec709,
            buffers: self.buffers,
        }
    }
}

/// A cheaply clonable video frame.
///
/// Use [Self::as_ref] to access it.
#[derive(Debug, Clone)]
pub struct OwnedVideoFrame {
    pub(super) format: VideoFormat,
    pub(super) metadata: VideoFrameMetadata,
    pub(super) frame_data: Bytes,
}

impl OwnedVideoFrame {
    pub fn metadata(&self) -> VideoFrameMetadata {
        self.metadata.clone()
    }

    pub fn as_ref<'a>(&'a self) -> VideoFrame<'a> {
        parse_frame(self.metadata.clone(), &self.frame_data, self.format)
    }
}

pub(super) fn parse_frame<'a>(
    metadata: VideoFrameMetadata,
    frame_data: &'a [u8],
    format: VideoFormat,
) -> VideoFrame<'a> {
    if format.contained_in(VideoFormats::MASK_H264 | VideoFormats::MASK_H265) {
        // -- H264 and H265
        let ParsedNalus {
            parsed_frame_type,
            buffers,
        } = parse_nalus(frame_data, format);

        VideoFrame {
            parsed_frame_type,
            metadata,
            buffers,
        }
    } else {
        // -- Av1
        VideoFrame {
            parsed_frame_type: match metadata.frame_type {
                FrameType::Idr => video::FrameType::Idr,
                _ => video::FrameType::PFrame,
            },
            metadata,
            buffers: smallvec![VideoFrameBuffer {
                buffer_type: BufferType::PicData,
                data: frame_data,
            }],
        }
    }
}
