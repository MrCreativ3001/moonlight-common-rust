use std::collections::BTreeMap;

use fec_rs::ReedSolomon;
use thiserror::Error;
use tracing::{debug, debug_span, trace, warn};

use crate::stream::{
    proto::video::{
        nal::{h264, h265},
        packet::{
            MAX_VIDEO_SHARDS_PER_FEC_BLOCK, RtpVideoHeader, VIDEO_FLAG_EXTENSION, VideoHeader,
            VideoHeaderFlags, fec_percentage_to_parity_shards,
        },
    },
    video::{BufferType, SupportedVideoFormats, VideoFormat, VideoFrameBuffer},
};

// TODO: https://github.com/moonlight-stream/moonlight-common-c/blob/2a5a1f3e8a57cbbb316ed7dfff3a3965c2e77d25/src/RtpVideoQueue.c#L253-L258
// TODO: what happens after frame loss: https://github.com/moonlight-stream/moonlight-common-c/blob/2a5a1f3e8a57cbbb316ed7dfff3a3965c2e77d25/src/VideoDepacketizer.c#L1128-L1156

#[derive(Debug, Error, Clone, PartialEq)]
pub enum VideoQueueError {
    #[error("a received video rtp packet doesn't have the configured packet size")]
    PacketInvalidSize,
}

#[derive(Debug, Clone)]
pub struct VideoDepayloaderConfig {
    /// This is the size of each packet minus the RTP_HEADER_SIZE (16 bytes).
    /// Each packet will have size [Self::packet_size] + 16.
    ///
    /// References:
    /// - Games on Whales docs: https://games-on-whales.github.io/wolf/stable/protocols/rtp-video.html#_rtp_packets
    pub packet_size: usize,
    pub format: VideoFormat,
}

#[derive(Debug, PartialEq)]
pub struct VideoFrame {
    pub frame_number: u32,
    /// The timestamp that the server sent.
    /// 90kHz clock time representation.
    ///
    /// References:
    /// - Moonlight common c: https://github.com/moonlight-stream/moonlight-common-c/blob/62687809b1f7410c3db4be2527503a54ae408d70/src/RtpVideoQueue.c#L157
    pub timestamp: u32,
    /// The buffers this frame consists of.
    ///
    /// Different codecs split buffers differently:
    /// - H264: each buffer starts with an annex b start code followed by a h264 nalu.
    /// - H265: each buffer starts with an annex b start code followed by a h265 nalu.
    /// - Av1: no specific point where they're being split
    // TODO: fix the lifetime
    pub buffers: Vec<VideoFrameBuffer<Vec<u8>>>,
}

struct Packet {
    frame_index: u32,
    timestamp: u32,
    fec_shard_index: u32,
    fec_total_data_shards: u32,
    fec_percentage: u32,
    data: Vec<u8>,
}

pub struct VideoDepayloader {
    config: VideoDepayloaderConfig,
    // TODO: try to avoid copying data by directly putting the packets into the correct position in this buffer
    current_frame_buffer: Vec<u8>,
    current_frame_index: u32,
    packets: BTreeMap<u16, Packet>,
}

pub(crate) fn create_video_reed_solomon(data_shards: usize, parity_shards: usize) -> ReedSolomon {
    #[allow(clippy::unwrap_used)]
    ReedSolomon::new(data_shards, parity_shards).unwrap()
}

// TODO: this looks funny: https://github.com/moonlight-stream/moonlight-common-c/blob/435bc6a5a4852c90cfb037de1378c0334ed36d8e/src/VideoDepacketizer.c#L849-L1124
// TODO: this should also handle decryption

impl VideoDepayloader {
    pub fn new(config: VideoDepayloaderConfig) -> Self {
        Self {
            config,
            current_frame_buffer: vec![],
            // Frame index starts at 1
            current_frame_index: 1,
            packets: Default::default(),
        }
    }

    /// This will skip to the next constructable frame that can be produced.
    pub fn skip_frames(&mut self) -> Result<Option<VideoFrame>, VideoQueueError> {
        let mut possible_frames = self
            .packets
            .values()
            .filter(|packet| packet.frame_index >= self.current_frame_index)
            .map(|packet| packet.frame_index)
            .collect::<Vec<_>>();
        possible_frames.sort();

        for frame_index in possible_frames {
            if let Some(output_frame) = self.try_construct_fec_block(frame_index)? {
                self.current_frame_index = frame_index;

                return Ok(Some(output_frame));
            }
        }

        Ok(None)
    }

    pub fn poll_frame(&mut self) -> Result<Option<VideoFrame>, VideoQueueError> {
        let mut output_frame = None;

        // Check if we can construct a frame
        if let Some(frame) = self.try_construct_fec_block(self.current_frame_index)? {
            output_frame = Some(frame);

            // TODO: increase current_frame_index and current_sequence_number
            self.current_frame_index += 1;
        }

        // Clear all old data
        self.packets
            .retain(|_, packet| packet.frame_index >= self.current_frame_index);

        Ok(output_frame)
    }

    fn try_construct_fec_block(
        &mut self,
        frame_index: u32,
    ) -> Result<Option<VideoFrame>, VideoQueueError> {
        // TODO: handle one frame in multiple fec blocks?

        let packets = self
            .packets
            .values_mut()
            .filter(|packet| packet.frame_index == frame_index)
            .collect::<Vec<_>>();

        if packets.is_empty() {
            return Ok(None);
        }

        // Grab data from packets
        let total_data_shards = packets[0].fec_total_data_shards as usize;
        let fec_percentage = packets[0].fec_percentage;
        let total_parity_shards =
            fec_percentage_to_parity_shards(total_data_shards, fec_percentage as usize);
        let timestamp = packets[0].timestamp;

        // Size of the payload of each packet. We checked the size in the handle_packet fn, so this cannot be different
        // TODO: this might get influenced by encryption??
        let payload_size = self.config.packet_size - VideoHeader::SIZE;

        #[cfg(debug_assertions)]
        {
            // Check the fec blocks for correctness
            for packet in packets.iter() {
                debug_assert_eq!(packet.fec_total_data_shards, total_data_shards as u32);
                debug_assert_eq!(packet.fec_percentage, fec_percentage);
                debug_assert_eq!(packet.timestamp, timestamp);
                debug_assert!(
                    (packet.fec_shard_index as usize) < total_data_shards + total_parity_shards
                );
            }
        }

        if packets.len() < total_data_shards {
            // We currently cannot produce a frame
            return Ok(None);
        }

        // -- Load all data shards into the current frame buffer and keep track of them
        let mut parity_shards_count = 0;
        let mut data_shards_count = 0;
        let mut data_shards = [false; MAX_VIDEO_SHARDS_PER_FEC_BLOCK];

        // Make sure the frame buffer is big enough
        self.current_frame_buffer
            .resize(total_data_shards * payload_size, 0);

        for packet in packets.iter() {
            if packet.fec_shard_index >= total_data_shards as u32 {
                // this is a fec shard -> we don't need them inside the frame
                parity_shards_count += 1;
                continue;
            }

            let index_start = packet.fec_shard_index as usize * payload_size;
            let index_end = (packet.fec_shard_index as usize + 1) * payload_size;

            self.current_frame_buffer[index_start..index_end].copy_from_slice(&packet.data);

            data_shards_count += 1;
            data_shards[packet.fec_shard_index as usize] = true;
        }

        // -- Build all shards, if there are parity shards
        if parity_shards_count > 0 {
            // TODO: use from_fn when stabilized
            let mut shards = Vec::with_capacity(total_data_shards + total_parity_shards);
            for _ in 0..(data_shards_count + total_parity_shards) {}

            // Insert all data shards
            for (shard_exists, chunk) in data_shards
                .iter()
                .zip(self.current_frame_buffer.chunks_mut(payload_size))
                .take(total_data_shards)
            {
                shards.push((chunk, *shard_exists));
            }

            // Insert all fec shards

            for packet in packets {
                // Only accept fec shards
                if packet.fec_shard_index < total_data_shards as u32 {
                    continue;
                }

                shards.resize_with(packet.fec_shard_index as usize, || (&mut [], false));
                shards[packet.fec_shard_index as usize] = (&mut packet.data, true);
            }

            // -- Reconstruct
            let reed_solomon = create_video_reed_solomon(total_data_shards, total_parity_shards);

            // TODO: reconstruct
            // reed_solomon.reconstruct_data(&mut shards).unwrap();
            todo!()
        }

        // -- Interpret frame
        let parse_frame_span = debug_span!("parse_frame");

        let frame = self.interpret_current_frame(frame_index, timestamp);

        drop(parse_frame_span);

        Ok(Some(frame))
    }

    /// Interprets the [Self::current_frame_buffer] and returns a VideoFrame
    ///
    /// Mostly the functionality of https://github.com/moonlight-stream/moonlight-common-c/blob/62687809b1f7410c3db4be2527503a54ae408d70/src/VideoDepacketizer.c#L743-L1156
    fn interpret_current_frame(&self, frame_number: u32, timestamp: u32) -> VideoFrame {
        // only h264 and h265 bitstreams are parsed
        if !self
            .config
            .format
            .contained_in(SupportedVideoFormats::H264 | SupportedVideoFormats::H265)
        {
            // TODO: encryption may change size
            let payload_size = self.config.packet_size - VideoHeader::SIZE;

            let buffers =
                self.current_frame_buffer
                    .chunks(payload_size)
                    .map(|x| VideoFrameBuffer {
                        buffer_type: BufferType::PicData,
                        data: x.to_owned(),
                    });

            return VideoFrame {
                frame_number,
                timestamp,
                buffers: buffers.collect(),
            };
        }

        // -- H264 and H265

        // Use a two to avoid conflicts with first byte being a one which would trigger a start code
        let mut start_code_window = [2u8; 4];

        let mut last_start_code = None;
        let mut buffers = Vec::new();

        // Add a buffer to the video frame buffer and finds out the buffer type
        let mut add_buffer = |nalu_start: usize, buffer: &[u8]| {
            let buffer_type = {
                if self.config.format.contained_in(SupportedVideoFormats::H264) {
                    if buffer.len() < nalu_start + 1 {
                        warn!("Couldn't read nal header because nalu is too short!");
                        trace!(frame = ?self.current_frame_buffer, buffer = ?buffer, nalu_start = nalu_start, "data");

                        BufferType::PicData
                    } else {
                        // H264 specific filtering
                        let nal_header = h264::NalHeader::parse([buffer[nalu_start]]);

                        nal_header.nal_unit_type.to_buffer_type()
                    }
                } else if self.config.format.contained_in(SupportedVideoFormats::H265) {
                    if buffer.len() < nalu_start + 2 {
                        warn!("Couldn't read nal header because nalu is too short!");
                        trace!(frame = ?self.current_frame_buffer, buffer = ?buffer, nalu_start = nalu_start, "data");

                        BufferType::PicData
                    } else {
                        // H265 specific filtering
                        let nal_header =
                            h265::NalHeader::parse([buffer[nalu_start], buffer[nalu_start + 1]]);

                        nal_header.nal_unit_type.to_buffer_type()
                    }
                } else {
                    unreachable!()
                }
            };

            buffers.push(VideoFrameBuffer {
                buffer_type,
                data: buffer.to_owned(),
            });
        };

        // Find annex b start codes
        for i in 0..self.current_frame_buffer.len() {
            start_code_window.rotate_left(1);
            start_code_window[3] = self.current_frame_buffer[i];

            let mut buffer = None;

            let mut nalu_offset = 0;
            if matches!(start_code_window, [_, 0, 0, 1]) {
                let new_start_code_len = if start_code_window[0] == 0 { 4 } else { 3 };

                let new_start_code_begin = i - (new_start_code_len - 1);
                if let Some((last_start_code_begin, last_start_code_len)) = last_start_code {
                    nalu_offset = last_start_code_len;
                    buffer = Some(
                        &self.current_frame_buffer[last_start_code_begin..new_start_code_begin],
                    );
                }
                last_start_code = Some((new_start_code_begin, new_start_code_len));
            }

            if let Some(buffer) = buffer {
                debug_assert_ne!(nalu_offset, 0);

                add_buffer(nalu_offset, buffer);
            }
        }

        if let Some((start_code_begin, start_code_len)) = last_start_code {
            add_buffer(
                start_code_len,
                &self.current_frame_buffer[start_code_begin..],
            );
        }

        VideoFrame {
            frame_number,
            timestamp,
            buffers,
        }
    }

    pub fn handle_packet(&mut self, packet: &[u8]) -> Result<(), VideoQueueError> {
        // Wolf impl: https://github.com/games-on-whales/wolf/blob/2c15d61107e48ca2fe3d350a703546aecb3eab78/src/moonlight-server/gst-plugin/video.hpp#L234-L268

        // TODO: for encrypted packets we should first verify the packet and then do any errors
        if packet.len() != RtpVideoHeader::SIZE + self.config.packet_size {
            debug!(
                got_len = packet.len(),
                expected_len = self.config.packet_size,
                "received packet with invalid size"
            );
            return Err(VideoQueueError::PacketInvalidSize);
        }

        #[allow(clippy::unwrap_used)]
        let rtp_header = RtpVideoHeader::deserialize(
            packet[0..RtpVideoHeader::SIZE]
                .as_array::<{ RtpVideoHeader::SIZE }>()
                .unwrap(),
        );

        #[allow(clippy::unwrap_used)]
        let video_header = VideoHeader::deserialize(
            packet[RtpVideoHeader::SIZE..(RtpVideoHeader::SIZE + VideoHeader::SIZE)]
                .as_array::<{ VideoHeader::SIZE }>()
                .unwrap(),
        );

        if video_header.frame_index < self.current_frame_index {
            // Drop this packet because we already skipped it
            return Ok(());
        }

        let data = &packet[(RtpVideoHeader::SIZE + VideoHeader::SIZE)..];

        trace!(rtp_header = ?rtp_header, video_header = ?video_header, "received video packet");

        // FLAG_EXTENSION is required for all supported versions of GFE: https://github.com/moonlight-stream/moonlight-common-c/blob/b126e481a195fdc7152d211def17190e3434bcce/src/RtpVideoQueue.c#L549-L550
        if rtp_header.header & VIDEO_FLAG_EXTENSION == 0 {
            // TODO: error
            todo!();
        }

        if !video_header
            .flags
            .contains(VideoHeaderFlags::CONTAINS_VIDEO_DATA)
        {
            // drop this packet because it doesn't contain any data
            return Ok(());
        }

        self.packets.insert(
            rtp_header.sequence_number,
            Packet {
                frame_index: video_header.frame_index,
                timestamp: rtp_header.timestamp,
                fec_shard_index: video_header.fec_info.shard_index,
                fec_total_data_shards: video_header.fec_info.data_shards_total,
                fec_percentage: video_header.fec_info.fec_percentage,
                data: data.to_vec(),
            },
        );

        Ok(())
    }
}
