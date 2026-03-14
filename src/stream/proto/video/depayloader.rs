use std::collections::BTreeMap;

use reed_solomon_erasure::galois_8::ReedSolomon;
use thiserror::Error;
use tracing::trace;

use crate::stream::{
    proto::video::packet::{
        MAX_VIDEO_SHARDS_PER_FEC_BLOCK, RtpVideoHeader, VIDEO_FLAG_EXTENSION, VideoHeader,
        VideoHeaderFlags, fec_percentage_to_parity_shards,
    },
    video::{VideoFormat, VideoFrameBuffer},
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
        sequence_number: u32,
    ) -> Result<Option<VideoFrame>, VideoQueueError> {
        // TODO: handle one frame in multiple fec blocks?

        let packets = self
            .packets
            .values_mut()
            .filter(|packet| packet.frame_index == sequence_number)
            .collect::<Vec<_>>();

        if packets.is_empty() {
            return Ok(None);
        }

        let total_data_shards = packets[0].fec_total_data_shards;
        let fec_percentage = packets[0].fec_total_data_shards;
        let timestamp = packets[0].timestamp;

        // Size of the payload of each packet. We checked the size in the handle_packet fn, so this cannot be different
        // TODO: this might get influenced by encryption??
        let payload_size = self.config.packet_size - RtpVideoHeader::SIZE - VideoHeader::SIZE;

        #[cfg(debug_assertions)]
        {
            // Check the fec blocks for correctness
            for packet in packets.iter() {
                debug_assert_eq!(packet.fec_total_data_shards, total_data_shards);
                debug_assert_eq!(packet.fec_percentage, fec_percentage);
                debug_assert_eq!(packet.timestamp, timestamp);
            }
        }

        if packets.len() < total_data_shards as usize {
            // We currently cannot produce a frame
            return Ok(None);
        }

        // -- Load all data shards into the current frame buffer and keep track of them
        let mut data_shards_count = 0;
        let mut data_shards = [false; MAX_VIDEO_SHARDS_PER_FEC_BLOCK];

        // Make sure the frame buffer is big enough
        self.current_frame_buffer
            .resize(total_data_shards as usize * payload_size, 0);

        for packet in packets.iter() {
            if packet.fec_shard_index >= total_data_shards {
                // this is a fec shard -> we don't need them inside the frame
                continue;
            }

            let index_start = packet.fec_shard_index as usize * payload_size;
            let index_end = (packet.fec_shard_index as usize + 1) * payload_size;

            self.current_frame_buffer[index_start..index_end].copy_from_slice(&packet.data);

            data_shards_count += 1;
            data_shards[packet.fec_shard_index as usize] = true;
        }

        // -- Build all shards
        let mut shards = Vec::new();

        // Insert all data shards
        for (shard_exists, chunk) in data_shards
            .iter()
            .zip(self.current_frame_buffer.chunks_mut(payload_size))
            .take(total_data_shards as usize)
        {
            shards.push((chunk, *shard_exists));
        }

        // Insert all fec shards

        for packet in packets {
            // Only accept fec shards
            if packet.fec_shard_index < total_data_shards {
                continue;
            }

            shards.resize_with(packet.fec_shard_index as usize, || (&mut [], false));
            shards[packet.fec_shard_index as usize] = (&mut packet.data, true);
        }

        // -- Reconstruct
        let parity_shards_count =
            fec_percentage_to_parity_shards(data_shards_count, fec_percentage as usize);
        let reed_solomon =
            create_video_reed_solomon(total_data_shards as usize, parity_shards_count);

        // TODO: remove unwrap
        reed_solomon.reconstruct_data(&mut shards).unwrap();

        // -- Interpret frame
        let frame = self.interpret_current_frame();

        Ok(Some(frame))
    }

    /// Interprets the [Self::current_frame_buffer] and returns a VideoFrame
    fn interpret_current_frame(&self) -> VideoFrame {
        todo!()
    }

    pub fn handle_packet(&mut self, packet: &[u8]) -> Result<(), VideoQueueError> {
        // Wolf impl: https://github.com/games-on-whales/wolf/blob/2c15d61107e48ca2fe3d350a703546aecb3eab78/src/moonlight-server/gst-plugin/video.hpp#L234-L268

        if packet.len() != self.config.packet_size {
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

        trace!("Rtp Header: {rtp_header:?}, Video Header: {video_header:?}");

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
