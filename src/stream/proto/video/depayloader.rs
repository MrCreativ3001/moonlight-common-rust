use std::collections::BTreeMap;

use reed_solomon_erasure::galois_8::ReedSolomon;
use thiserror::Error;
use tracing::trace;

use crate::stream::{
    proto::video::packet::{RtpVideoHeader, VIDEO_FLAG_EXTENSION, VideoHeader, VideoHeaderFlags},
    video::{BufferType, VideoFrameBuffer},
};

// TODO: https://github.com/moonlight-stream/moonlight-common-c/blob/2a5a1f3e8a57cbbb316ed7dfff3a3965c2e77d25/src/RtpVideoQueue.c#L253-L258
// TODO: what happens after frame loss: https://github.com/moonlight-stream/moonlight-common-c/blob/2a5a1f3e8a57cbbb316ed7dfff3a3965c2e77d25/src/VideoDepacketizer.c#L1128-L1156

#[derive(Debug, Error, Clone, PartialEq)]
pub enum VideoQueueError {
    #[error("the received video rtp packet was too short")]
    PacketTooShort,
}

#[derive(Debug, Clone)]
pub struct VideoDepayloaderConfig {
    pub packet_size: usize,
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
    data: Vec<u8>,
}

pub struct VideoDepayloader {
    config: VideoDepayloaderConfig,
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
        let timestamp = packets[0].timestamp;

        #[cfg(debug_assertions)]
        {
            // Check the fec block for correctness
            for packet in packets.iter() {
                debug_assert_eq!(packet.fec_total_data_shards, total_data_shards);
                debug_assert_eq!(packet.timestamp, timestamp);
            }
        }

        if packets.len() < total_data_shards as usize {
            // We currently cannot produce a frame
            return Ok(None);
        }

        // TODO: calculate this using fec percentage
        let parity_shards = packets.len() - total_data_shards as usize;

        // Check if we need fec reconstruction
        if parity_shards == 0 {
            // We don't need fec reconstruction and we can directly submit our data

            // TODO: split them at the annex b prefix
            let mut buffers = Vec::new();
            for packet in &packets {
                buffers.push(VideoFrameBuffer {
                    buffer_type: BufferType::PicData,
                    data: packet.data.clone(),
                });
            }

            return Ok(Some(VideoFrame {
                frame_number: sequence_number,
                timestamp,
                buffers,
            }));
        }

        // Do fec reconstruction
        // TODO: don't use a vec
        // TODO: don't clone the vec
        let mut shards = Vec::new();
        for shard_index in 0..total_data_shards {
            let shard = packets
                .iter()
                .find(|packet| packet.fec_shard_index == shard_index);

            if let Some(shard) = shard {
                shards.push(Some(shard.data.clone()));
            } else {
                shards.push(None);
            }
        }

        let reed_solomon = create_video_reed_solomon(total_data_shards as usize, parity_shards);
        // TODO: remove unwrap
        reed_solomon.reconstruct_data(&mut shards).unwrap();

        // TODO: fix
        // Ok(Some(VideoFrame {
        //     frame_number: sequence_number,
        //     timestamp,
        //     buffers: shards.into_iter().flatten().collect::<Vec<_>>(),
        // }))
        todo!()
    }

    pub fn handle_packet(&mut self, packet: &[u8]) -> Result<(), VideoQueueError> {
        // Wolf impl: https://github.com/games-on-whales/wolf/blob/2c15d61107e48ca2fe3d350a703546aecb3eab78/src/moonlight-server/gst-plugin/video.hpp#L234-L268

        if packet.len() < RtpVideoHeader::SIZE + VideoHeader::SIZE {
            return Err(VideoQueueError::PacketTooShort);
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
                data: data.to_vec(),
            },
        );

        Ok(())
    }
}
