use std::{array, collections::BTreeMap, ops::Range, time::Duration};

use bytes::BytesMut;
use fec_rs::ReedSolomon;
use thiserror::Error;
use tracing::{debug, trace, warn};

use crate::{
    ServerVersion,
    stream::{
        proto::{
            fec::ArrayShard,
            video::{
                frame::{OwnedVideoFrame, VideoFrame, VideoFrameMetadata, parse_frame},
                packet::{
                    MAX_VIDEO_FEC_BLOCKS, MAX_VIDEO_SHARDS_PER_FEC_BLOCK,
                    RtpVideoHeader, VIDEO_FLAG_EXTENSION, VideoFrameHeader, VideoHeader,
                    VideoHeaderFlags, fec_percentage_to_parity_shards,
                },
            },
        },
        video::{
            FrameIndex, VideoFormat, VideoFormats,
        },
    },
};

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
    /// This means PAYLOAD_SIZE = ACTUAL_PACKET_SIZE - RTP_HEADER_SIZE - VIDEO_HEADER_SIZE = ACTUAL_PACKET_SIZE - 32.
    ///
    /// References:
    /// - Games on Whales docs: <https://games-on-whales.github.io/wolf/stable/protocols/rtp-video.html#_rtp_packets>
    pub packet_size: usize,
    pub format: VideoFormat,
    /// The version of the server.
    pub server_version: ServerVersion,
}

#[derive(Debug, Clone, PartialEq)]
pub struct VideoDepayloaderFrameStatus {
    /// The index of this frame.
    pub frame_index: FrameIndex,
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
    pub fn is_frame_constructed(&self) -> bool {
        let Some(total_data_packets) = self.total_data_packets else {
            return false;
        };

        self.current_block_index >= self.total_blocks - 1
            && self.received_data_packets + self.received_parity_packets >= total_data_packets - 1
    }
}

#[derive(Debug)]
struct Packet {
    rtp_header: RtpVideoHeader,
    video_header: VideoHeader,
    data: Vec<u8>,
}

/// A frame will be constructed block by block.
/// For efficiency received data packets will directly be copied into the buffer if the received packet is the same block.
/// If it's not the same buffer it'll wait until that block is complete because it might still be unknown how big this previous block is.
///
/// The is finished if [Self::last_block_index] > [Self::current_block]
#[derive(Debug)]
struct Frame {
    current_block: u8,
    current_block_buffer_offset: usize,
    current_block_available_shards: [bool; MAX_VIDEO_SHARDS_PER_FEC_BLOCK],
    current_block_total_data_shards: Option<usize>,
    /// if [Self::current_block_total_data_shards] is present is this present too
    current_block_fec_percentage: Option<usize>,
    last_block_index: u8,
    timestamp: u32,
    buffer: BytesMut,
}

#[derive(Debug)]
pub struct VideoDepayloader {
    config: VideoDepayloaderConfig,
    packets: BTreeMap<u16, Packet>,
    frames: BTreeMap<FrameIndex, Frame>,
}

pub(crate) fn create_video_reed_solomon(data_shards: usize, parity_shards: usize) -> ReedSolomon {
    #[allow(clippy::unwrap_used)]
    ReedSolomon::new(data_shards, parity_shards).unwrap()
}

// TODO: how to handle fec? https://github.com/moonlight-stream/moonlight-common-c/blob/62687809b1f7410c3db4be2527503a54ae408d70/src/ControlStream.c#L455-L469

impl VideoDepayloader {
    /// Creates a new VideoDepayloader
    ///
    /// Our FEC recovery code doesn't work properly until Gen 5
    /// <https://github.com/moonlight-stream/moonlight-common-c/blob/2a5a1f3e8a57cbbb316ed7dfff3a3965c2e77d25/src/RtpVideoQueue.c#L253-L258>
    pub fn new(config: VideoDepayloaderConfig) -> Self {
        Self {
            config,
            packets: Default::default(),
            frames: Default::default(),
        }
    }

    fn payload_len(&self) -> usize {
        self.config.packet_size - VideoHeader::SIZE
    }

    pub fn is_frame_known(&self, frame_index: FrameIndex) -> bool {
        self.frames.contains_key(&frame_index)
    }
    pub fn is_frame_available(&self, frame_index: FrameIndex) -> bool {
        self.constructed_frame(frame_index).is_some()
    }

    /// Iterates over all known frames of this depayloader.
    /// This will also contain non constructable frames.
    pub fn known_frames(&self) -> impl Iterator<Item = FrameIndex> + use<'_> {
        self.frames.keys().cloned()
    }
    /// Return all currently constructed and available frames.
    /// Every returned frame can query [Self::frame_metadata] or [Self::frame] without an error.
    pub fn available_frames(&self) -> impl Iterator<Item = FrameIndex> + use<'_> {
        self.frames
            .iter()
            .filter(|(_, frame)| frame.current_block > frame.last_block_index)
            .map(|(frame_index, _)| *frame_index)
    }

    fn constructed_frame(&self, frame_index: FrameIndex) -> Option<&Frame> {
        let frame = self.frames.get(&frame_index)?;

        if frame.current_block <= frame.last_block_index {
            return None;
        }

        Some(frame)
    }
    /// Get frame metadata about the frame.
    /// This will only return metadata after the frame was fully received.
    ///
    /// You should prefer this over [Self::frame] if you only need the [VideoFrameMetadata].
    pub fn frame_metadata(&self, frame_index: FrameIndex) -> Option<VideoFrameMetadata> {
        let frame = self.constructed_frame(frame_index)?;

        let (metadata, _) = self.parse_frame_header(
            frame_index,
            rtp_timestamp_to_duration(frame.timestamp),
            &frame.buffer,
        );

        Some(metadata)
    }
    /// Get the fully parsed and finished frame if it is fully reconstructible.
    pub fn frame(&self, frame_index: FrameIndex) -> Option<VideoFrame<'_>> {
        let frame = self.constructed_frame(frame_index)?;

        let (metadata, data_range) = self.parse_frame_header(
            frame_index,
            rtp_timestamp_to_duration(frame.timestamp),
            &frame.buffer,
        );

        Some(parse_frame(
            metadata,
            &frame.buffer[data_range],
            self.config.format,
        ))
    }

    /// This will also discard the frame.
    pub fn take_frame(&mut self, frame_index: FrameIndex) -> Option<OwnedVideoFrame> {
        if !self.is_frame_available(frame_index) {
            return None;
        }
        let frame = self
            .frames
            .remove(&frame_index)
            .expect("the frame should exist in this state");

        let (metadata, data_range) = self.parse_frame_header(
            frame_index,
            rtp_timestamp_to_duration(frame.timestamp),
            &frame.buffer,
        );

        // Shorten the frame to the range
        let full_frame = frame.buffer.freeze();

        Some(OwnedVideoFrame {
            format: self.config.format,
            metadata,
            frame_data: full_frame.slice(data_range),
        })
    }

    /// Discard everything that is currently known about the frame.
    /// This can be called for frames that are in construction or already constructed frames.
    pub fn discard_frame(&mut self, frame_index: FrameIndex) {
        self.frames.remove(&frame_index);

        self.packets
            .retain(|_, packet| packet.video_header.frame_index != *frame_index);
    }

    /// Parses the frame header and returns the [VideoFrameMetadata] and the actual frame data.
    ///
    /// Mostly the functionality of https://github.com/moonlight-stream/moonlight-common-c/blob/62687809b1f7410c3db4be2527503a54ae408d70/src/VideoDepacketizer.c#L743-L1156
    fn parse_frame_header(
        &self,
        frame_index: FrameIndex,
        timestamp: Duration,
        mut full_frame: &[u8],
    ) -> (VideoFrameMetadata, Range<usize>) {
        // parse the frame header
        // https://github.com/moonlight-stream/moonlight-common-c/blob/7b026e77be62175104640e7e722b758df6d3d0d7/src/VideoDepacketizer.c#L855-L972

        if full_frame.len() < 8 {
            // TODO: what now?
            todo!();
        }
        #[allow(clippy::unwrap_used)]
        let frame_header = VideoFrameHeader::deserialize(full_frame[0..8].try_into().unwrap());

        // Truncate the buffer to the len if we're not using H264 / H265.
        // This is required for non H264 / H265.
        // https://github.com/moonlight-stream/moonlight-common-c/blob/7b026e77be62175104640e7e722b758df6d3d0d7/src/VideoDepacketizer.c#L905-L912
        if !self
            .config
            .format
            .contained_in(VideoFormats::MASK_H264 | VideoFormats::MASK_H265)
        {
            let payload_len = self.payload_len();
            let last_payload_start = full_frame.len().saturating_sub(payload_len);

            full_frame =
                &full_frame[0..last_payload_start + frame_header.last_payload_len as usize];
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

        debug_assert!(full_frame.len() > frame_header_len);

        let metadata = VideoFrameMetadata {
            frame_index,
            frame_type: frame_header.frame_type,
            host_processing_latency,
            timestamp,
        };

        (metadata, frame_header_len..full_frame.len())
    }

    fn try_construct_fec_block(
        &mut self,
        frame_index: FrameIndex,
        block_index: u8,
    ) -> Result<bool, VideoDepayloaderError> {
        let payload_len = self.payload_len();

        let frame = if let Some(frame) = self.frames.get_mut(&frame_index) {
            frame
        } else {
            let Some(frame_packet) = self
                .packets
                .values()
                .find(|packet| packet.video_header.frame_index == *frame_index)
            else {
                return Ok(false);
            };

            self.frames.insert(
                frame_index,
                Frame {
                    timestamp: frame_packet.rtp_header.timestamp,
                    current_block: 0,
                    current_block_buffer_offset: 0,
                    current_block_available_shards: [false; _],
                    current_block_total_data_shards: None,
                    current_block_fec_percentage: None,
                    last_block_index: frame_packet.video_header.multi_fec_blocks.last_block_index,
                    buffer: Default::default(),
                },
            );

            // This cannot fail because this element was just inserted
            #[allow(clippy::unwrap_used)]
            self.frames.get_mut(&frame_index).unwrap()
        };

        // this frame would already be finished
        if block_index > frame.last_block_index {
            return Ok(false);
        }

        // The block index will always be incremented after a full block
        if block_index != frame.current_block {
            return Ok(false);
        }

        // -- iterate over all packets to find data and fec packets of this block
        self.packets.retain(|_, packet| {
            // filter for frame index and block index
            if packet.video_header.frame_index != *frame_index {
                return true;
            }
            if packet.video_header.multi_fec_blocks.current_block != block_index {
                return true;
            }

            if packet.video_header.multi_fec_blocks.last_block_index >= MAX_VIDEO_FEC_BLOCKS as u8 {
                warn!(
                    rtp_header = ?packet.rtp_header,
                    video_header = ?packet.video_header,
                    got_last_block_index = packet.video_header.multi_fec_blocks.last_block_index,
                    max_blocks = MAX_VIDEO_SHARDS_PER_FEC_BLOCK,
                    "dropping invalid video packet: last block index is higher than maximum allowed"
                );
                return false;
            }

            debug_assert_eq!(
                packet.rtp_header.timestamp, frame.timestamp,
                "timestamps of packets in one frame must match be always equal!"
            );

            // mark shard as available if it's in the valid shard range
            if packet.video_header.fec_info.shard_index as usize >= MAX_VIDEO_SHARDS_PER_FEC_BLOCK {
                warn!(
                    rtp_header = ?packet.rtp_header,
                    video_header = ?packet.video_header,
                    got_shard_index = packet.video_header.fec_info.shard_index,
                    max_shards = MAX_VIDEO_SHARDS_PER_FEC_BLOCK,
                    "dropping invalid video packet: fec shard index is higher than maximum allowed"
                );
                return false;
            }

            // update available shards
            frame.current_block_available_shards
                [packet.video_header.fec_info.shard_index as usize] = true;

            // update total data shards
            if let Some(total_data_shards) = frame.current_block_total_data_shards {
                debug_assert_eq!(
                    total_data_shards, packet.video_header.fec_info.data_shards_total as usize,
                    "all packets in one block must have the total_data_shards"
                );
                debug_assert_eq!(
                    {
                        #[allow(clippy::unwrap_used)]
                        frame.current_block_fec_percentage.unwrap()
                    },
                    packet.video_header.fec_info.fec_percentage as usize,
                    "all packets in one block must have the total_data_shards"
                );
            } else {
                frame.current_block_total_data_shards =
                    Some(packet.video_header.fec_info.data_shards_total as usize);
                frame.current_block_fec_percentage =
                    Some(packet.video_header.fec_info.fec_percentage as usize);
            }

            if packet.video_header.fec_info.shard_index
                >= packet.video_header.fec_info.data_shards_total
            {
                // fec shard, those must be retained for reconstruction
                return true;
            }

            // copy data shard into the frame buffer
            let data = &packet.data;

            let block_shard_offset =
                payload_len * packet.video_header.fec_info.shard_index as usize;

            let shard_start = frame.current_block_buffer_offset + block_shard_offset;
            let shard_end = shard_start + payload_len;
            if frame.buffer.len() < shard_end {
                frame.buffer.resize(shard_end, 0);
            }

            frame.buffer[shard_start..shard_end].copy_from_slice(data);

            false
        });

        // -- try to finish the block by either having it already or reconstructing it using reed solomon
        let Some(total_data_shards) = frame.current_block_total_data_shards else {
            return Ok(false);
        };
        let fec_percentage = frame.current_block_fec_percentage.expect("frame.current_block_total_shards is present but frame.current_block_fec_percentage is absent. This should be impossible!");
        let total_parity_shards =
            fec_percentage_to_parity_shards(total_data_shards, fec_percentage);

        // see how many shards we have
        let mut data_shards = 0;
        let mut parity_shards = 0;
        for shard_index in frame
            .current_block_available_shards
            .iter()
            .enumerate()
            .filter_map(|(shard_index, present)| present.then_some(shard_index))
        {
            if shard_index < total_data_shards {
                // data shard
                data_shards += 1;
            } else {
                // fec shard
                parity_shards += 1;
            }

            if data_shards + parity_shards >= total_data_shards {
                // it's possible to construct a block, no need to continue
                break;
            }
        }

        if data_shards + parity_shards < total_data_shards {
            // cannot construct a block currently
            return Ok(false);
        }

        // -- use reed solomon reconstruction if required
        if parity_shards > 0 {
            trace!(frame_index = ?frame_index, block_index = block_index, data_shards = data_shards, parity_shards = parity_shards, total_data_shards = total_data_shards, total_parity_shards = total_parity_shards, "reconstructing frame with reed solomon");

            // make sure the frame buffer is big enough to fit all data shards in this block into it, this is important for fec reconstruction later
            let full_block_len = total_data_shards * payload_len;
            let required_buffer_len = frame.current_block_buffer_offset + full_block_len;
            if frame.buffer.len() < required_buffer_len {
                frame.buffer.resize(required_buffer_len, 0);
            }

            let mut shards: [_; MAX_VIDEO_SHARDS_PER_FEC_BLOCK] = array::from_fn(|_| ArrayShard {
                len: None,
                array: &mut [],
            });

            // fill all data shards
            for ((shard_index, present), chunk) in frame
                .current_block_available_shards
                .iter()
                .take(total_data_shards)
                .enumerate()
                .zip(frame.buffer[frame.current_block_buffer_offset..].chunks_mut(payload_len))
            {
                shards[shard_index] = ArrayShard {
                    len: present.then_some(chunk.len()),
                    array: chunk,
                };
            }

            // fill all required parity shards
            for (_, packet) in self.packets.iter_mut().filter(|(_, packet)| {
                packet.video_header.frame_index == *frame_index
                    && packet.video_header.multi_fec_blocks.current_block == block_index
            }) {
                // all packets that are data shards or have invalid shard index should already be filtered out, see retain above
                shards[packet.video_header.fec_info.shard_index as usize] = ArrayShard {
                    len: Some(packet.data.len()),
                    array: &mut packet.data,
                };
            }

            trace!(shards = ?shards[0..(total_data_shards + total_parity_shards)], "data shard");

            // reconstruct
            let reed_solomon = create_video_reed_solomon(total_data_shards, total_parity_shards);

            reed_solomon
                .reconstruct_data(&mut shards[0..(total_data_shards + total_parity_shards)])?;
        }

        // prepare frame for next block
        frame.current_block += 1;
        frame.current_block_available_shards = [false; _];
        frame.current_block_total_data_shards = None;
        frame.current_block_buffer_offset += total_data_shards * payload_len;

        // drop all packets related to this frame, if the frame is complete
        if frame.last_block_index > frame.current_block {
            self.packets
                .retain(|_, packet| packet.video_header.frame_index != *frame_index);
        }

        Ok(true)
    }

    pub fn handle_packet(&mut self, packet: &[u8]) -> Result<(), VideoDepayloaderError> {
        // Wolf impl: https://github.com/games-on-whales/wolf/blob/2c15d61107e48ca2fe3d350a703546aecb3eab78/src/moonlight-server/gst-plugin/video.hpp#L234-L268

        // TODO: encryption support?
        // let data = if let Some(aes_key) = self.aes_key.as_ref() {
        //     // https://github.com/moonlight-stream/moonlight-common-c/blob/62687809b1f7410c3db4be2527503a54ae408d70/src/VideoStream.c#L184-L222

        //     let encryption_header = EncryptedVideoHeader::deserialize(
        //         data[0..EncryptedVideoHeader::SIZE]
        //             .as_array::<{ EncryptedVideoHeader::SIZE }>()
        //             .unwrap(),
        //     );

        //     let mut decrypted = vec![0; data.len() - EncryptedVideoHeader::SIZE];

        //     let size = self.crypto_backend.decrypt(
        //         CipherAlgorithm::Aes128Gcm,
        //         &aes_key,
        //         &encryption_header.iv,
        //         Some(&encryption_header.tag),
        //         &data[32..],
        //         &mut decrypted,
        //     )?;
        //     decrypted.resize(size, 0);

        //     decrypted
        // } else {
        //     data.to_vec()
        // };

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

        // TODO: auto drop packets that have a higher frame index that we have

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

        let frame_index = video_header.frame_index;
        let mut block_index = video_header.multi_fec_blocks.current_block;

        self.packets.insert(
            rtp_header.sequence_number,
            Packet {
                rtp_header,
                video_header,
                data: data.to_vec(),
            },
        );

        // try to construct blocks with the new packet
        if let Some(frame) = self.frames.get(&FrameIndex(frame_index)) {
            block_index = block_index.min(frame.current_block);
        }

        while self.try_construct_fec_block(FrameIndex(frame_index), block_index)? {
            block_index = block_index.saturating_add(1);
        }

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
