// Interesting
// Sunshine payloading: https://github.com/LizardByte/Sunshine/blob/69d7b6df27375c622db7e329f87dcd885efad76f/src/stream.cpp#L1268-L1590
// Wolf payloading: https://github.com/games-on-whales/wolf/blob/2c15d61107e48ca2fe3d350a703546aecb3eab78/src/moonlight-server/gst-plugin/video.hpp

use std::{array, collections::VecDeque};

use thiserror::Error;

use crate::stream::proto::video::{
    depayloader::create_video_reed_solomon,
    packet::{
        FrameType, MAX_VIDEO_SHARDS_PER_FEC_BLOCK, RtpVideoHeader, VIDEO_FLAG_EXTENSION,
        VideoFecInfo, VideoFrameHeader, VideoHeader, VideoHeaderFlags, VideoMultiFecBlocks,
        fec_percentage_from, fec_percentage_to_parity_shards,
    },
};

pub struct VideoPayloaderFecConfig {
    pub min_required_fec_packets: usize,
    pub fec_percentage: usize,
}

pub struct VideoPayloaderConfig {
    pub packet_size: usize,
    pub fec: Option<VideoPayloaderFecConfig>,
}

// TODO: parse host processing latency: https://github.com/LizardByte/Sunshine/blob/69d7b6df27375c622db7e329f87dcd885efad76f/src/stream.cpp#L1329-L1340

#[derive(Debug, Error, PartialEq)]
pub enum VideoPayloaderError {
    // TODO: queue filled up?
}

pub struct VideoPayloader {
    payload_len: usize,
    fec_config: Option<VideoPayloaderFecConfig>,
    sequence_number: u16,
    frame_index: u32,
    unused: Vec<Vec<u8>>,
    packet_queue_used_front: bool,
    packet_queue: VecDeque<Vec<u8>>,
}

fn header_size() -> usize {
    // TODO: does this also need the encryption header if present?
    RtpVideoHeader::SIZE + VideoHeader::SIZE
}

impl VideoPayloader {
    pub fn new(config: VideoPayloaderConfig) -> Self {
        assert!(
            config.packet_size > header_size(),
            "The packet size must be larger than the size of the headers!"
        );

        Self {
            payload_len: config.packet_size - header_size(),
            fec_config: config.fec,
            sequence_number: 0,
            frame_index: 0,
            packet_queue_used_front: false,
            unused: Vec::default(),
            packet_queue: VecDeque::default(),
        }
    }

    /// This will set the fec config used when generating NEW frames.
    pub fn set_fec_config(&mut self, config: Option<VideoPayloaderFecConfig>) {
        self.fec_config = config;
    }

    // TODO: take a VideoFrame as an argument with borrowed values?
    pub fn push_frame(
        &mut self,
        timestamp: u32,
        frame_type: FrameType,
        frame: &[u8],
    ) -> Result<(), VideoPayloaderError> {
        let full_frame_len = VideoFrameHeader::SIZE + frame.len();
        let last_payload_len = if full_frame_len.is_multiple_of(self.payload_len) {
            self.payload_len
        } else {
            full_frame_len % self.payload_len
        };

        let frame_header = VideoFrameHeader {
            header_type: 0x01,
            frame_type,
            unknown: [0; _],
            last_payload_len: last_payload_len as u16,
            unknown2: [0; _],
        };

        // TODO: multi fec blocks?
        // TODO: look for data_shards max: https://github.com/games-on-whales/wolf/blob/2c15d61107e48ca2fe3d350a703546aecb3eab78/src/moonlight-server/gst-plugin/video.hpp#L335-L336

        let packets = self.generate_fec_block(
            self.sequence_number,
            Some(frame_header),
            VideoMultiFecBlocks {
                block_index: 0,
                current_block: 0,
                unused: 0,
            },
            timestamp,
            frame,
        )?;

        self.sequence_number = self.sequence_number.wrapping_add(packets as u16);
        self.frame_index = self.frame_index.wrapping_add(1);

        Ok(())
    }

    /// Generates a fec block and returns the amount of packets that were produced into self.packet_queue
    fn generate_fec_block(
        &mut self,
        sequence_number: u16,
        frame_header: Option<VideoFrameHeader>,
        multi_fec_blocks: VideoMultiFecBlocks,
        timestamp: u32,
        block_data: &[u8],
    ) -> Result<usize, VideoPayloaderError> {
        let header_size = header_size();
        let packet_size = self.payload_len + header_size;

        let mut current_sequence_number = sequence_number;
        let mut current_packet_count = 0;

        // Create fec info
        let full_frame_len =
            block_data.len() + frame_header.map(|_| VideoFrameHeader::SIZE).unwrap_or(0);
        let data_shards_count = full_frame_len.div_ceil(self.payload_len);
        let mut parity_shards_count = 0;

        let fec_percentage = if let Some(VideoPayloaderFecConfig {
            min_required_fec_packets,
            mut fec_percentage,
        }) = self.fec_config
            && (fec_percentage > 0 || min_required_fec_packets > 0)
        {
            parity_shards_count =
                fec_percentage_to_parity_shards(data_shards_count, fec_percentage);
            if parity_shards_count < min_required_fec_packets {
                parity_shards_count = min_required_fec_packets;
                fec_percentage = fec_percentage_from(data_shards_count, parity_shards_count);
            }

            fec_percentage
        } else {
            0
        };

        // The position we're currently inside of block_data
        // Must substract VideoFrameHeader::SIZE because this includes the frame header
        let mut block_position = 0;

        while block_position < block_data.len() + VideoFrameHeader::SIZE {
            // TODO: how to handle failure?
            let mut packet = self.dequeue_packet().unwrap();

            // Serialize header
            let rtp_header = RtpVideoHeader {
                header: 0x80 | VIDEO_FLAG_EXTENSION,
                packet_type: 0,
                sequence_number: current_sequence_number,
                ssrc: 0,
                timestamp,
                reserved: [0; _],
            };

            // This is allowed
            #[allow(clippy::unwrap_used)]
            rtp_header.serialize(packet[0..RtpVideoHeader::SIZE].as_mut_array().unwrap());

            let video_header = VideoHeader {
                stream_packet_index: (current_sequence_number as u32) << 8,
                frame_index: self.frame_index,
                flags: VideoHeaderFlags::from_index(current_packet_count, data_shards_count, true),
                reserved: 0,
                multi_fec_flags: 0x10,
                multi_fec_blocks,
                fec_info: VideoFecInfo {
                    data_shards_total: data_shards_count as u32,
                    shard_index: current_packet_count as u32,
                    fec_percentage: fec_percentage as u32,
                    unused: 0,
                },
            };

            // This is allowed
            #[allow(clippy::unwrap_used)]
            video_header.serialize(
                packet[RtpVideoHeader::SIZE..(RtpVideoHeader::SIZE + VideoHeader::SIZE)]
                    .as_mut_array()
                    .unwrap(),
            );

            // Serialize payload
            if block_position == 0
                && let Some(frame_header) = &frame_header
            {
                // We're the first payload, we need the VideoFrameHeader

                // This cannot panic because of the range
                #[allow(clippy::unwrap_used)]
                frame_header.serialize(
                    packet[header_size..(header_size + VideoFrameHeader::SIZE)]
                        .as_mut_array()
                        .unwrap(),
                );

                let frame_end = self.payload_len.min(block_data.len()) - VideoFrameHeader::SIZE;

                packet[(header_size + VideoFrameHeader::SIZE)..packet_size]
                    .copy_from_slice(&block_data[0..frame_end]);

                block_position += VideoFrameHeader::SIZE + frame_end;
            } else {
                let frame_start = block_position - VideoFrameHeader::SIZE;
                let mut frame_end = frame_start + self.payload_len;
                let mut payload_end = packet_size;

                // Pad with zeros if we're the last packet
                // Important for the fec generation
                if frame_end > block_data.len() {
                    payload_end = packet_size - (frame_end - block_data.len());
                    frame_end = block_data.len();

                    packet[payload_end..].fill(0);
                }

                packet[header_size..payload_end]
                    .copy_from_slice(&block_data[frame_start..frame_end]);

                block_position += frame_end - frame_start;
            }

            self.packet_queue.push_back(packet);

            current_sequence_number = current_sequence_number.wrapping_add(1);
            current_packet_count += 1;
        }

        // Generate parity shards if required
        if parity_shards_count > 0 {
            let reed_solomon = create_video_reed_solomon(data_shards_count, parity_shards_count);

            // Copy data shards to create parity shards
            let mut data_shards: [&[u8]; MAX_VIDEO_SHARDS_PER_FEC_BLOCK] =
                [&[]; MAX_VIDEO_SHARDS_PER_FEC_BLOCK];
            for (i, packet) in self
                .packet_queue
                .iter()
                .skip(self.packet_queue.len() - data_shards_count)
                .enumerate()
            {
                data_shards[i] = &packet[header_size..];
            }

            // Generate Fec shards
            // TODO: don't use vec
            let mut parity_shards: [Vec<u8>; MAX_VIDEO_SHARDS_PER_FEC_BLOCK] =
                array::from_fn(|_| vec![0; self.payload_len]);

            // This cannot fail because we control all shards
            #[allow(clippy::unwrap_used)]
            reed_solomon
                .encode_sep(
                    &data_shards[0..data_shards_count],
                    &mut parity_shards[0..parity_shards_count],
                )
                .unwrap();

            // Generate fec packets
            for parity_shard in &parity_shards[0..parity_shards_count] {
                // TODO: how to handle failure?
                let mut packet = self.dequeue_packet().unwrap();

                let rtp_header = RtpVideoHeader {
                    header: 0x80 | VIDEO_FLAG_EXTENSION,
                    packet_type: 0x00,
                    sequence_number: current_sequence_number,
                    timestamp: 0,
                    ssrc: 0,
                    reserved: [0; _],
                };

                // This is allowed
                #[allow(clippy::unwrap_used)]
                rtp_header.serialize(packet[0..RtpVideoHeader::SIZE].as_mut_array().unwrap());

                let video_header = VideoHeader {
                    stream_packet_index: (current_sequence_number as u32) << 8,
                    frame_index: self.frame_index,
                    flags: VideoHeaderFlags::empty(),
                    reserved: 0,
                    multi_fec_flags: 0x10,
                    multi_fec_blocks,
                    fec_info: VideoFecInfo {
                        data_shards_total: data_shards_count as u32,
                        shard_index: current_packet_count as u32,
                        fec_percentage: fec_percentage as u32,
                        unused: 0,
                    },
                };

                // This is allowed
                #[allow(clippy::unwrap_used)]
                video_header.serialize(
                    packet[RtpVideoHeader::SIZE..(RtpVideoHeader::SIZE + VideoHeader::SIZE)]
                        .as_mut_array()
                        .unwrap(),
                );

                packet[header_size..].copy_from_slice(parity_shard);

                self.packet_queue.push_back(packet);

                current_sequence_number = current_sequence_number.wrapping_add(1);
                current_packet_count += 1;
            }
        }

        Ok(current_packet_count)
    }

    fn dequeue_packet(&mut self) -> Result<Vec<u8>, VideoPayloaderError> {
        if let Some(vec) = self.unused.pop() {
            return Ok(vec);
        }

        Ok(vec![0; self.payload_len + header_size()])
    }

    pub fn poll_packet(&mut self) -> Result<Option<&[u8]>, VideoPayloaderError> {
        if self.packet_queue_used_front {
            let packet = self.packet_queue.pop_front();
            // Insert packet
            if let Some(packet) = packet {
                self.unused.push(packet);
            }
        } else {
            self.packet_queue_used_front = true;
        }

        let packet = self.packet_queue.front();

        Ok(packet.map(|x| x.as_slice()))
    }
}
