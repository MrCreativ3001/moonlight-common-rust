use smallvec::SmallVec;
use tracing::{trace, warn};

use crate::stream::video::{self, BufferType, VideoFormat, VideoFormats, VideoFrameBuffer};

pub mod h264;
pub mod h265;

pub struct ParsedNalus<'a> {
    pub parsed_frame_type: video::FrameType,
    pub buffers: SmallVec<[VideoFrameBuffer<&'a [u8]>; 4]>,
}

pub fn parse_nalus<'a>(frame_data: &'a [u8], format: VideoFormat) -> ParsedNalus<'a> {
    // -- H264 and H265
    // only h264 and h265 bitstreams are parsed
    let mut parsed_frame_type = video::FrameType::PFrame;

    // parse the frame type ourselves
    // See https://github.com/moonlight-stream/moonlight-common-c/blob/7b026e77be62175104640e7e722b758df6d3d0d7/src/VideoDepacketizer.c#L311-L339

    // Use a two to avoid conflicts with first byte being a one which would trigger a start code
    let mut start_code_window = [2u8; 4];

    let mut last_start_code = None;
    let mut buffers = SmallVec::new();

    // Add a buffer to the video frame buffer and finds out the buffer type
    let mut add_buffer = |nalu_start: usize, buffer: &'a [u8]| {
        let buffer_type = {
            if format.contained_in(VideoFormats::MASK_H264) {
                if buffer.len() < nalu_start + 1 {
                    warn!("Couldn't read nal header because nalu is too short!");
                    trace!(frame = ?frame_data, buffer = ?buffer, nalu_start = nalu_start, "data");

                    BufferType::PicData
                } else {
                    // H264 specific filtering
                    let nal_header = h264::NalHeader::parse([buffer[nalu_start]]);

                    // See frame type definition for info
                    if matches!(nal_header.nal_unit_type, h264::NalUnitType::CodedSliceIDR) {
                        parsed_frame_type = video::FrameType::Idr;
                    }

                    nal_header.nal_unit_type.to_buffer_type()
                }
            } else if format.contained_in(VideoFormats::MASK_H265) {
                if buffer.len() < nalu_start + 2 {
                    warn!("Couldn't read nal header because nalu is too short!");
                    trace!(frame = ?frame_data, buffer = ?buffer, nalu_start = nalu_start, "data");

                    BufferType::PicData
                } else {
                    // H265 specific filtering
                    let nal_header =
                        h265::NalHeader::parse([buffer[nalu_start], buffer[nalu_start + 1]]);

                    // See frame type definition for info
                    if matches!(
                        nal_header.nal_unit_type,
                        h265::NalUnitType::BlaWLp
                            | h265::NalUnitType::BlaWRadl
                            | h265::NalUnitType::BlaNLp
                            | h265::NalUnitType::IdrWRadl
                            | h265::NalUnitType::IdrNLp
                            | h265::NalUnitType::CraNut
                    ) {
                        parsed_frame_type = video::FrameType::Idr;
                    }

                    nal_header.nal_unit_type.to_buffer_type()
                }
            } else {
                unreachable!()
            }
        };

        buffers.push(VideoFrameBuffer {
            buffer_type,
            data: buffer,
        });
    };

    // Find annex b start codes
    for i in 0..frame_data.len() {
        start_code_window.rotate_left(1);
        start_code_window[3] = frame_data[i];

        let mut buffer = None;

        let mut nalu_offset = 0;
        if matches!(start_code_window, [_, 0, 0, 1]) {
            let new_start_code_len = if start_code_window[0] == 0 { 4 } else { 3 };

            let new_start_code_begin = i - (new_start_code_len - 1);
            if let Some((last_start_code_begin, last_start_code_len)) = last_start_code {
                nalu_offset = last_start_code_len;
                buffer = Some(&frame_data[last_start_code_begin..new_start_code_begin]);
            }
            last_start_code = Some((new_start_code_begin, new_start_code_len));
        }

        if let Some(buffer) = buffer {
            debug_assert_ne!(nalu_offset, 0);

            add_buffer(nalu_offset, buffer);
        }
    }

    if let Some((start_code_begin, start_code_len)) = last_start_code {
        add_buffer(start_code_len, &frame_data[start_code_begin..]);
    }

    ParsedNalus {
        parsed_frame_type,
        buffers,
    }
}
