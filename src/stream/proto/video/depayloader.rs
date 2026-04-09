use std::{collections::BTreeMap, time::Duration};

use fec_rs::ReedSolomon;
use thiserror::Error;
use tracing::{Level, debug, instrument, trace, trace_span, warn};

use crate::{
    ServerVersion,
    stream::{
        proto::{
            fec::ArrayShard,
            video::{
                nal::{h264, h265},
                packet::{
                    FrameType, MAX_VIDEO_SHARDS_PER_FEC_BLOCK, RtpVideoHeader,
                    VIDEO_FLAG_EXTENSION, VideoFrameHeader, VideoHeader, VideoHeaderFlags,
                    fec_percentage_to_parity_shards,
                },
            },
        },
        video::{BufferType, SupportedVideoFormats, VideoFormat, VideoFrameBuffer},
    },
};

// TODO: what happens after frame loss: https://github.com/moonlight-stream/moonlight-common-c/blob/2a5a1f3e8a57cbbb316ed7dfff3a3965c2e77d25/src/VideoDepacketizer.c#L1128-L1156

#[derive(Debug, Error, Clone, PartialEq)]
pub enum VideoDepayloaderError {
    #[error("a received video rtp packet doesn't have the configured packet size")]
    PacketInvalidSize,
    #[error("reed solomon: {0}")]
    ReedSolomon(#[from] fec_rs::Error),
}

#[derive(Debug, Clone)]
pub struct VideoDepayloaderConfig {
    /// This is the size of each packet minus the RTP_HEADER_SIZE (16 bytes).
    /// Each packet will have size [Self::packet_size] + 16.
    ///
    /// The actual packet consists of RTP_HEADER_SIZE + VIDEO_HEADER_SIZE + PAYLOAD_SIZE.
    /// This means PAYLOAD_SIZE = PACKET_SIZE - RTP_HEADER_SIZE - VIDEO_HEADER_SIZE = PACKET_SIZE - 32.
    ///
    /// References:
    /// - Games on Whales docs: https://games-on-whales.github.io/wolf/stable/protocols/rtp-video.html#_rtp_packets
    pub packet_size: usize,
    pub format: VideoFormat,
    /// The version of the server.
    pub server_version: ServerVersion,
}

#[derive(Debug, Clone, PartialEq)]
pub struct VideoDepayloaderStatus {
    /// The number of the current frame.
    ///
    /// If [None] it's currently searching for the next frame and will produce any frame without any order.
    pub current_frame_index: Option<u32>,
    /// The highest seen frame index.
    pub highest_seen_frame_index: Option<u32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct VideoDepayloaderFrameStatus {
    /// The index of this frame.
    pub frame_index: u32,
    /// The timestamp of this frame, if it exists
    pub timestamp: Duration,
    /// The received data packets in the [Self::current_block_index]
    pub received_data_packets: usize,
    /// The received parity packets in the [Self::current_block_index]
    pub received_parity_packets: usize,
    /// The total data packets in the [Self::current_block_index]
    /// None if it's still unknown how many data packets are in this block.
    pub total_data_packets: Option<usize>,
    /// The current block in this [Self::frame_index]
    pub current_block_index: usize,
    /// The total blocks in this [Self::frame_index]
    pub total_blocks: usize,
}

impl VideoDepayloaderFrameStatus {
    /// If the frame is constructable by the [VideoDepayloader].
    pub fn is_constructable(&self) -> bool {
        let Some(total_data_packets) = self.total_data_packets else {
            return false;
        };

        self.current_block_index >= self.total_blocks - 1
            && self.received_data_packets + self.received_parity_packets >= total_data_packets - 1
    }
}

#[derive(Debug, PartialEq)]
pub struct VideoFrame {
    pub frame_index: u32,
    /// Type of this frame.
    /// - For H264 and H265 this will be parsed using the nalus from the bitstream.
    /// - For other codecs (Av1) this will be the value from the server
    pub frame_type: FrameType,
    /// The timestamp that the server sent.
    /// 90kHz clock time representation.
    ///
    /// References:
    /// - Moonlight common c: https://github.com/moonlight-stream/moonlight-common-c/blob/62687809b1f7410c3db4be2527503a54ae408d70/src/RtpVideoQueue.c#L157
    pub timestamp: Duration,
    /// The processing latency of the host
    ///
    /// References:
    /// - https://github.com/moonlight-stream/moonlight-common-c/blob/7b026e77be62175104640e7e722b758df6d3d0d7/src/Limelight.h#L151-L155
    pub host_processing_latency: Option<Duration>,
    /// The buffers this frame consists of.
    ///
    /// Different codecs split buffers differently:
    /// - H264: each buffer starts with an annex b start code followed by a h264 nalu.
    /// - H265: each buffer starts with an annex b start code followed by a h265 nalu.
    /// - Av1: no specific point where they're being split
    // TODO: fix the lifetime
    pub buffers: Vec<VideoFrameBuffer<Vec<u8>>>,
}

#[derive(Debug, PartialEq)]
pub enum VideoDepayloaderOutput {
    /// The video depayloader produced a frame.
    /// This also contains other information regarding the frame.
    ///
    /// See also:
    /// - moonlight loss stats: https://github.com/moonlight-stream/moonlight-common-c/blob/435bc6a5a4852c90cfb037de1378c0334ed36d8e/src/ControlStream.c#L1364-L1464
    Frame { frame: VideoFrame },
}

#[derive(Debug)]
struct Packet {
    frame_index: u32,
    timestamp: u32,
    fec_shard_index: u32,
    fec_total_data_shards: u32,
    fec_percentage: u32,
    data: Vec<u8>,
}

#[derive(Debug)]
pub struct VideoDepayloader {
    config: VideoDepayloaderConfig,
    // TODO: try to avoid copying data by directly putting the packets into the correct position in this buffer
    current_frame_buffer: Vec<u8>,
    current_frame_index: Option<u32>,
    packets: BTreeMap<u16, Packet>,
}

pub(crate) fn create_video_reed_solomon(data_shards: usize, parity_shards: usize) -> ReedSolomon {
    #[allow(clippy::unwrap_used)]
    ReedSolomon::new(data_shards, parity_shards).unwrap()
}

// TODO: this looks funny: https://github.com/moonlight-stream/moonlight-common-c/blob/435bc6a5a4852c90cfb037de1378c0334ed36d8e/src/VideoDepacketizer.c#L849-L1124
// TODO: this should also handle decryption

// TODO: how to handle fec? https://github.com/moonlight-stream/moonlight-common-c/blob/62687809b1f7410c3db4be2527503a54ae408d70/src/ControlStream.c#L455-L469
// TODO: encryption? https://github.com/moonlight-stream/moonlight-common-c/blob/62687809b1f7410c3db4be2527503a54ae408d70/src/VideoStream.c#L184-L222

impl VideoDepayloader {
    /// Creates a new VideoDepayloader
    ///
    /// Our FEC recovery code doesn't work properly until Gen 5
    /// https://github.com/moonlight-stream/moonlight-common-c/blob/2a5a1f3e8a57cbbb316ed7dfff3a3965c2e77d25/src/RtpVideoQueue.c#L253-L258
    pub fn new(config: VideoDepayloaderConfig) -> Self {
        Self {
            config,
            current_frame_buffer: vec![],
            current_frame_index: None,
            packets: Default::default(),
        }
    }

    /// Sets the state of this depayloader.
    ///
    /// # See also
    /// - [Self::poll_output]
    pub fn set_current_frame_index(
        &mut self,
        current_frame: Option<u32>,
    ) -> Result<(), VideoDepayloaderError> {
        self.current_frame_index = current_frame;

        Ok(())
    }

    pub fn status(&self) -> VideoDepayloaderStatus {
        let current_frame_index = self.current_frame_index;
        let highest_seen_frame_index = self.packets.values().map(|x| x.frame_index).max();

        VideoDepayloaderStatus {
            current_frame_index,
            highest_seen_frame_index,
        }
    }
    pub fn frame_status(&self, frame_index: u32) -> Option<VideoDepayloaderFrameStatus> {
        let mut received_data_packets = 0;
        let mut received_parity_packets = 0;
        let mut total_data_packets = None;
        let mut timestamp = None;

        for (_, packet) in self
            .packets
            .iter()
            .filter(|packet| packet.1.frame_index == frame_index)
        {
            if packet.fec_shard_index < packet.fec_total_data_shards {
                received_data_packets += 1;
            } else {
                received_parity_packets += 1;
            }
            total_data_packets = Some(packet.fec_total_data_shards as usize);

            if timestamp.is_none() {
                timestamp = Some(rtp_timestamp_to_duration(packet.timestamp));
            }

            #[cfg(debug_assertions)]
            if let Some(timestamp) = timestamp {
                debug_assert_eq!(
                    timestamp,
                    rtp_timestamp_to_duration(packet.timestamp),
                    "all packets of a single video frame must have the same timestamp!"
                );
            }
        }

        Some(VideoDepayloaderFrameStatus {
            frame_index,
            timestamp: timestamp?,
            received_data_packets,
            received_parity_packets,
            total_data_packets,
            current_block_index: 0,
            total_blocks: 1,
        })
    }

    /// If [VideoDepayloaderStatus::current_frame_index] is:
    /// - Present: It'll try to construct the [current_frame_index](VideoDepayloader::current_frame_index) and increment this value automatically on a produced frame.
    /// - Absent: It'll try to construct any constructable frames in an ascending order. This doesn't mean that frames will be produced in this order (e.g. late frame arrival).
    ///
    /// The depayloader itself won't set it's state.
    /// Use the [set_current_frame_index](Self::set_current_frame_index) function with [Some] to set the depayloader to a synced state to let it produce frames in order.
    pub fn poll_output(&mut self) -> Result<Option<VideoFrame>, VideoDepayloaderError> {
        let mut output = None;

        // TODO: more aggressively remove packets that are likely not being used, especially when unsynced

        // -- Check if we can construct a frame
        if let Some(current_frame_index) = self.current_frame_index {
            // If synced try to construct the next frame
            if let Some(frame) = self.try_construct_fec_block(current_frame_index)? {
                output = Some(frame);

                #[allow(clippy::unwrap_used)]
                let current_frame_index = self.current_frame_index.as_mut().unwrap();
                *current_frame_index += 1;
            }
        } else {
            // If not synced try to produce any frame
            let known_frames = self.packets.values().fold(Vec::new(), |mut value, packet| {
                if !value.contains(&packet.frame_index) {
                    value.push(packet.frame_index);
                }
                value
            });

            // All frames should be sorted because the sequence number and frame_index are both ascending, but at a different rate
            debug_assert!(
                known_frames.is_sorted(),
                "All frames should be sorted because the sequence number and frame_index are both ascending, but at a different rate"
            );

            for frame_index in known_frames {
                if let Some(frame) = self.try_construct_fec_block(frame_index)? {
                    output = Some(frame);

                    // remove frame from packets, the current frame is in the buffer
                    self.packets
                        .retain(|_, packet| packet.frame_index != frame_index);
                    break;
                }
            }
        }

        // -- Clear all old data
        if let Some(current_frame_index) = self.current_frame_index {
            self.packets
                .retain(|_, packet| packet.frame_index >= current_frame_index);
        }

        Ok(output)
    }

    fn try_construct_fec_block(
        &mut self,
        frame_index: u32,
    ) -> Result<Option<VideoFrame>, VideoDepayloaderError> {
        // TODO: handle one frame in multiple fec blocks?

        let packets = self
            .packets
            .values_mut()
            .filter(|packet| packet.frame_index == frame_index)
            .collect::<Vec<_>>();

        if packets.is_empty() {
            return Ok(None);
        }

        // -- Grab data from packets
        let total_data_shards = packets[0].fec_total_data_shards as usize;
        let fec_percentage = packets[0].fec_percentage;
        let total_parity_shards =
            fec_percentage_to_parity_shards(total_data_shards, fec_percentage as usize);
        let total_shards = total_data_shards + total_parity_shards;
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

        // -- Check if a frame can be produced
        if packets.len() < total_data_shards {
            // We currently cannot produce a frame
            return Ok(None);
        }

        // -- Create a shard index to packet quick access
        self.current_frame_buffer.clear();
        self.current_frame_buffer
            .resize(total_shards * payload_size, 0);

        let mut quick_shard_to_packet = [None; MAX_VIDEO_SHARDS_PER_FEC_BLOCK];
        for packet in packets.iter() {
            quick_shard_to_packet[packet.fec_shard_index as usize] = Some(packet);
        }

        // -- Reconstruct all data using previously generated quick access
        let mut data_shards_count = 0;
        let mut parity_shards_count = 0;

        // TODO: avoid heap alloc?
        let mut shards = Vec::with_capacity(MAX_VIDEO_SHARDS_PER_FEC_BLOCK);
        for (shard_index, shard_buffer) in self
            .current_frame_buffer
            .chunks_exact_mut(payload_size)
            .enumerate()
        {
            if let Some(packet) = &quick_shard_to_packet[shard_index] {
                if packet.fec_shard_index < packet.fec_total_data_shards {
                    data_shards_count += 1;
                } else {
                    parity_shards_count += 1;
                }

                // Shard is present -> copy data from shard, mark as present in len field
                shard_buffer.copy_from_slice(&packet.data);

                shards.push(ArrayShard {
                    len: Some(payload_size),
                    array: shard_buffer,
                });
            } else {
                // Shard is absent -> mark as absent in len field
                shards.push(ArrayShard {
                    len: None,
                    array: shard_buffer,
                });
            }
        }

        // See if fec reconstruction is needed?
        if parity_shards_count > 0 {
            // -- Reconstruct data using all shards
            let reed_solomon = ReedSolomon::new(total_data_shards, total_parity_shards)?;
            reed_solomon.reconstruct_data(&mut shards)?;
        }

        // -- Interpret frame
        let parse_frame_span = trace_span!("parse_frame");

        let frame =
            self.interpret_current_frame(frame_index, timestamp, total_data_shards * payload_size);

        drop(parse_frame_span);

        Ok(Some(frame))
    }

    /// Interprets the [Self::current_frame_buffer] and returns a VideoFrame
    ///
    /// Mostly the functionality of https://github.com/moonlight-stream/moonlight-common-c/blob/62687809b1f7410c3db4be2527503a54ae408d70/src/VideoDepacketizer.c#L743-L1156
    #[instrument(level = Level::TRACE, skip(self), fields(buffer_len = self.current_frame_buffer.len()))]
    fn interpret_current_frame(
        &mut self,
        frame_number: u32,
        timestamp: u32,
        last_payload_start: usize,
    ) -> VideoFrame {
        // parse the frame header
        // https://github.com/moonlight-stream/moonlight-common-c/blob/7b026e77be62175104640e7e722b758df6d3d0d7/src/VideoDepacketizer.c#L855-L972

        if self.current_frame_buffer.len() < 8 {
            // TODO: what now?
            todo!();
        }
        #[allow(clippy::unwrap_used)]
        let frame_header =
            VideoFrameHeader::deserialize(self.current_frame_buffer[0..8].try_into().unwrap());

        // Truncate the buffer to the len if we're not using H264 / H265.
        // This is required for non H264 / H265.
        // https://github.com/moonlight-stream/moonlight-common-c/blob/7b026e77be62175104640e7e722b758df6d3d0d7/src/VideoDepacketizer.c#L905-L912
        if !self
            .config
            .format
            .contained_in(SupportedVideoFormats::MASK_H264 | SupportedVideoFormats::MASK_H265)
        {
            self.current_frame_buffer
                .truncate(last_payload_start + frame_header.last_payload_len as usize);
        }

        // Skip the rest of the header
        // https://github.com/moonlight-stream/moonlight-common-c/blob/7b026e77be62175104640e7e722b758df6d3d0d7/src/VideoDepacketizer.c#L914-L972
        let frame_header_len;
        if self.config.server_version >= ServerVersion::new(7, 1, 450, 0) {
            // >= 7.1.450 uses 2 different header lengths based on the first byte:
            // 0x01 indicates an 8 byte header
            // 0x81 indicates a 44 byte header
            if frame_header.header_type == 0x01 {
                frame_header_len = 8;
            } else {
                debug_assert_eq!(frame_header.header_type, 0x81);
                frame_header_len = 44;
            }
        } else if self.config.server_version >= ServerVersion::new(7, 1, 446, 0) {
            // [7.1.446, 7.1.450) uses 2 different header lengths based on the first byte:
            // 0x01 indicates an 8 byte header
            // 0x81 indicates a 41 byte header
            if frame_header.header_type == 0x01 {
                frame_header_len = 8;
            } else {
                debug_assert_eq!(frame_header.header_type, 0x81);
                frame_header_len = 41;
            }
        } else if self.config.server_version >= ServerVersion::new(7, 1, 415, 0) {
            // [7.1.415, 7.1.446) uses 2 different header lengths based on the first byte:
            // 0x01 indicates an 8 byte header
            // 0x81 indicates a 24 byte header
            if frame_header.header_type == 0x01 {
                frame_header_len = 8;
            } else {
                debug_assert_eq!(frame_header.header_type, 0x81);
                frame_header_len = 24;
            }
        } else if self.config.server_version >= ServerVersion::new(7, 1, 350, 0) {
            // [7.1.350, 7.1.415) should use the 8 byte header again
            frame_header_len = 8;
        } else if self.config.server_version >= ServerVersion::new(7, 1, 320, 0) {
            // [7.1.320, 7.1.350) should use the 12 byte frame header
            frame_header_len = 12;
        } else if self.config.server_version >= ServerVersion::new(5, 0, 0, 0) {
            // [5.x, 7.1.320) should use the 8 byte header
            frame_header_len = 8;
        } else {
            // Other versions don't have a frame header at all
            frame_header_len = 0;
        }

        trace!(frame_header = ?frame_header, frame_header_len = frame_header_len, "frame header");

        let host_processing_latency = (frame_header.host_processing_latency != 0).then_some(
            Duration::from_micros(frame_header.host_processing_latency as u64 * 100),
        );

        debug_assert!(self.current_frame_buffer.len() > frame_header_len);

        // Make sure to skip frame header
        let frame_data = &self.current_frame_buffer[frame_header_len..];

        // TODO: what about the other frame types?
        let mut frame_type = frame_header.frame_type;

        // only h264 and h265 bitstreams are parsed
        if !self
            .config
            .format
            .contained_in(SupportedVideoFormats::MASK_H264 | SupportedVideoFormats::MASK_H265)
        {
            return VideoFrame {
                frame_index: frame_number,
                // Trust the server frame type
                frame_type,
                timestamp: rtp_timestamp_to_duration(timestamp),
                host_processing_latency,
                buffers: vec![VideoFrameBuffer {
                    buffer_type: BufferType::PicData,
                    data: frame_data.to_vec(),
                }],
            };
        } else {
            // parse the frame type ourselves
            // See https://github.com/moonlight-stream/moonlight-common-c/blob/7b026e77be62175104640e7e722b758df6d3d0d7/src/VideoDepacketizer.c#L311-L339
            frame_type = FrameType::PFrame;
        }

        // -- H264 and H265

        // Use a two to avoid conflicts with first byte being a one which would trigger a start code
        let mut start_code_window = [2u8; 4];

        let mut last_start_code = None;
        let mut buffers = Vec::new();

        // Add a buffer to the video frame buffer and finds out the buffer type
        let mut add_buffer = |nalu_start: usize, buffer: &[u8]| {
            let buffer_type = {
                if self
                    .config
                    .format
                    .contained_in(SupportedVideoFormats::MASK_H264)
                {
                    if buffer.len() < nalu_start + 1 {
                        warn!("Couldn't read nal header because nalu is too short!");
                        trace!(frame = ?frame_data, buffer = ?buffer, nalu_start = nalu_start, "data");

                        BufferType::PicData
                    } else {
                        // H264 specific filtering
                        let nal_header = h264::NalHeader::parse([buffer[nalu_start]]);

                        // See frame type definition for info
                        if matches!(nal_header.nal_unit_type, h264::NalUnitType::CodedSliceIDR) {
                            frame_type = FrameType::Idr;
                        }

                        nal_header.nal_unit_type.to_buffer_type()
                    }
                } else if self
                    .config
                    .format
                    .contained_in(SupportedVideoFormats::MASK_H265)
                {
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
                            frame_type = FrameType::Idr;
                        }

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

        VideoFrame {
            frame_index: frame_number,
            frame_type,
            timestamp: rtp_timestamp_to_duration(timestamp),
            host_processing_latency,
            buffers,
        }
    }

    pub fn handle_packet(&mut self, packet: &[u8]) -> Result<(), VideoDepayloaderError> {
        // Wolf impl: https://github.com/games-on-whales/wolf/blob/2c15d61107e48ca2fe3d350a703546aecb3eab78/src/moonlight-server/gst-plugin/video.hpp#L234-L268

        // TODO: for encrypted packets we should first verify the packet and then do any errors
        if packet.len() != RtpVideoHeader::SIZE + self.config.packet_size {
            debug!(
                got_len = packet.len(),
                expected_len = self.config.packet_size,
                "received packet with invalid size"
            );
            return Err(VideoDepayloaderError::PacketInvalidSize);
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

        if let Some(current_frame_index) = self.current_frame_index
            && video_header.frame_index < current_frame_index
        {
            // Drop this packet because we already skipped it
            return Ok(());
        }

        let data = &packet[(RtpVideoHeader::SIZE + VideoHeader::SIZE)..];

        trace!(rtp_header = ?rtp_header, video_header = ?video_header, "received video packet");

        // FLAG_EXTENSION is required for all supported versions of GFE: https://github.com/moonlight-stream/moonlight-common-c/blob/b126e481a195fdc7152d211def17190e3434bcce/src/RtpVideoQueue.c#L549-L550
        debug_assert!(rtp_header.header & VIDEO_FLAG_EXTENSION != 0);

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

fn rtp_timestamp_to_duration(ts: u32) -> Duration {
    // 90 kHz -> 90,000 ticks per second
    const RTP_FREQ: u64 = 90_000;

    // Separate integer and fractional parts for precision
    let secs = ts as u64 / RTP_FREQ;
    let frac_ticks = ts as u64 % RTP_FREQ;

    let nanos = (frac_ticks * 1_000_000_000) / RTP_FREQ;

    Duration::new(secs, nanos as u32)
}
