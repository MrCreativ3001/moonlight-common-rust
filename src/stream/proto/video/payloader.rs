// Interesting
// Sunshine payloading: https://github.com/LizardByte/Sunshine/blob/69d7b6df27375c622db7e329f87dcd885efad76f/src/stream.cpp#L1268-L1590
// Wolf payloading: https://github.com/games-on-whales/wolf/blob/2c15d61107e48ca2fe3d350a703546aecb3eab78/src/moonlight-server/gst-plugin/video.hpp

use std::{array, collections::VecDeque, sync::Arc, time::Duration};

use bytes::{Bytes, BytesMut};
use tracing::{Level, instrument, trace, warn};

use crate::{
    ServerVersion,
    crypto::disabled::DisabledCryptoBackend,
    error::Error,
    stream::{
        AesIv, AesKey,
        proto::{
            DynCryptoBackend,
            crypto::CryptoBackend,
            video::{
                depayloader::create_video_reed_solomon,
                packet::{
                    EncryptedVideoHeader, MAX_VIDEO_SHARDS_PER_FEC_BLOCK, RtpVideoHeader,
                    VIDEO_FLAG_EXTENSION, VideoFecInfo, VideoFrameHeader, VideoHeader,
                    VideoHeaderExtraFlags, VideoHeaderFlags, VideoMultiFecBlocks,
                    fec_percentage_from, fec_percentage_to_parity_shards, video_header_size,
                },
            },
        },
        video::ServerFrameType,
    },
};

#[cfg(test)]
#[path = "./payloader_tests.rs"]
mod payloader_tests;

#[cfg(all(test, feature = "rustcrypto"))]
#[path = "payloader_tests_encrypted.rs"]
mod tests_encrypted;

/// With a fec_percentage of 255, if payload is broken up into more than a 100 data_shards
/// it will generate greater than DATA_SHARDS_MAX shards and FEC will fail to encode.
///
/// References:
/// - <https://github.com/games-on-whales/wolf/blob/2c15d61107e48ca2fe3d350a703546aecb3eab78/src/moonlight-server/gst-plugin/video.hpp#L335-L336>
pub const MAX_DATA_SHARDS_PER_BLOCK: usize = 100;

#[derive(Debug)]
pub struct VideoPayloaderFecConfig {
    pub min_required_fec_packets: usize,
    pub fec_percentage: usize,
}

#[derive(Debug)]
pub struct VideoPayloaderConfig {
    // TODO: the depayloader has different version, implement those here
    pub server_version: ServerVersion,
    /// This is the size of the resulting udp packet.
    pub packet_size: usize,
    /// The maximum amount of data shards per fec block.
    /// It's recommended to set this to 90.
    ///
    /// If it is lower than 1 or higher than [MAX_DATA_SHARDS_PER_BLOCK] it'll be clamped to those bounds.
    ///
    /// See also
    /// - [MAX_DATA_SHARDS_PER_BLOCK]
    ///
    /// References:
    /// - <https://github.com/games-on-whales/wolf/blob/2c15d61107e48ca2fe3d350a703546aecb3eab78/src/moonlight-server/gst-plugin/video.hpp#L335-L336>
    pub max_data_shards_per_block: usize,
    pub fec: Option<VideoPayloaderFecConfig>,
    pub encryption: Option<(AesKey, AesIv)>,
}

struct Encryption {
    aes_key: AesKey,
    iv_counter: u64,
}

pub struct VideoPayloader {
    crypto_backend: DynCryptoBackend,
    payload_len: usize,
    max_data_shards_per_block: usize,
    fec_config: Option<VideoPayloaderFecConfig>,
    sequence_number: u16,
    frame_index: u32,
    packet_queue: VecDeque<Bytes>,
    /// If some encryption is enabled
    encryption: Option<Encryption>,
    tmp_buffer: Vec<u8>,
}

fn header_size(encrypted: bool) -> usize {
    (if encrypted {
        EncryptedVideoHeader::SIZE
    } else {
        0
    }) + RtpVideoHeader::SIZE
        + VideoHeader::SIZE
}

impl VideoPayloader {
    pub fn new_unencrypted(config: VideoPayloaderConfig) -> Self {
        Self::new(config, Arc::new(DisabledCryptoBackend))
    }

    #[instrument(level = Level::DEBUG)]
    pub fn new(mut config: VideoPayloaderConfig, crypto_backend: DynCryptoBackend) -> Self {
        // TODO: don't panic, but use errors instead
        let header_size = video_header_size(config.encryption.is_some());
        assert!(
            config.packet_size > header_size,
            "The packet size must be larger than the size of the headers!"
        );

        let payload_len = config.packet_size - header_size;
        assert!(
            payload_len >= 8,
            "The packet must've at least 8 bytes of payload for the VideoFrameHeader"
        );

        config.max_data_shards_per_block = config.max_data_shards_per_block.clamp(1, 100);

        Self {
            crypto_backend,
            payload_len,
            max_data_shards_per_block: config.max_data_shards_per_block,
            fec_config: config.fec,
            sequence_number: 0,
            // Frame Index Starts at 1!
            frame_index: 1,
            packet_queue: VecDeque::default(),
            tmp_buffer: vec![0; header_size + payload_len],
            encryption: config.encryption.map(|(aes_key, _)| Encryption {
                aes_key,
                iv_counter: 0,
            }),
        }
    }

    /// This will set the fec config used when generating NEW frames.
    pub fn set_fec_config(&mut self, config: Option<VideoPayloaderFecConfig>) {
        self.fec_config = config;
    }

    #[instrument(level = Level::TRACE, skip(self, frame), fields(frame_len = %frame.len()))]
    pub fn push_frame(
        &mut self,
        timestamp: u32,
        host_processing_latency: Option<Duration>,
        frame_type: ServerFrameType,
        frame: &[u8],
    ) -> Result<(), Error> {
        let full_frame_len = VideoFrameHeader::SIZE + frame.len();
        let last_payload_len = if full_frame_len.is_multiple_of(self.payload_len) {
            self.payload_len
        } else {
            full_frame_len % self.payload_len
        };

        let frame_header = VideoFrameHeader {
            header_type: 0x01,
            frame_type,
            host_processing_latency: host_processing_latency
                .map(|host_processing_latency| (host_processing_latency.as_micros() / 100) as u16)
                .unwrap_or(0),
            last_payload_len: last_payload_len as u16,
            reserved: [0; _],
        };

        let data_shards_total = full_frame_len.div_ceil(self.payload_len);
        let blocks = data_shards_total.div_ceil(self.max_data_shards_per_block);

        let mut start = 0;
        for block in 0..blocks {
            let end = if block == 0 {
                start + (self.payload_len * self.max_data_shards_per_block) - VideoFrameHeader::SIZE
            } else {
                start + (self.payload_len * self.max_data_shards_per_block)
            }
            .min(frame.len());

            trace!(%block, %start, %end, "generating fec block");

            let packets = self.generate_fec_block(
                self.sequence_number,
                if block == 0 { Some(frame_header) } else { None },
                VideoMultiFecBlocks {
                    last_block_index: (blocks - 1) as u8,
                    current_block: block as u8,
                    unused: 0,
                },
                timestamp,
                &frame[start..end],
            );

            start = end;

            self.sequence_number = self.sequence_number.wrapping_add(packets as u16);
        }

        self.frame_index = self.frame_index.wrapping_add(1);

        Ok(())
    }

    /// Generates a fec block and returns the amount of packets that were produced into self.packet_queue
    #[instrument(level = Level::TRACE, skip_all)]
    fn generate_fec_block(
        &mut self,
        sequence_number: u16,
        frame_header: Option<VideoFrameHeader>,
        multi_fec_blocks: VideoMultiFecBlocks,
        timestamp: u32,
        block_data: &[u8],
    ) -> usize {
        let header_size = header_size(self.encryption.is_some());
        let packet_size = self.payload_len + header_size;

        let frame_header_opt = if frame_header.is_some() {
            VideoFrameHeader::SIZE
        } else {
            0
        };

        let mut current_sequence_number = sequence_number;
        let mut current_packet_count = 0;

        // Create fec info
        let full_frame_len = block_data.len() + frame_header_opt;
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

        while block_position < block_data.len() + frame_header_opt {
            let mut packet = BytesMut::zeroed(packet_size);

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
                flags: VideoHeaderFlags::from_index(
                    current_packet_count,
                    data_shards_count,
                    true,
                    multi_fec_blocks.current_block as usize,
                    multi_fec_blocks.last_block_index as usize,
                ),
                extra_flags: VideoHeaderExtraFlags::empty(),
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

            #[cfg(debug_assertions)]
            trace!(?rtp_header, ?video_header, "packet");

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

                let frame_end = self
                    .payload_len
                    .min(block_data.len())
                    .saturating_sub(VideoFrameHeader::SIZE);

                packet[(header_size + VideoFrameHeader::SIZE)
                    ..(header_size + VideoFrameHeader::SIZE + frame_end)]
                    .copy_from_slice(&block_data[0..frame_end]);

                block_position += VideoFrameHeader::SIZE + frame_end;
            } else {
                let frame_start = block_position - frame_header_opt;
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

            self.packet_queue.push_back(packet.freeze());

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
                let rtp_header = RtpVideoHeader {
                    header: 0x80 | VIDEO_FLAG_EXTENSION,
                    packet_type: 0x00,
                    sequence_number: current_sequence_number,
                    timestamp: 0,
                    ssrc: 0,
                    reserved: [0; _],
                };

                let video_header = VideoHeader {
                    stream_packet_index: (current_sequence_number as u32) << 8,
                    frame_index: self.frame_index,
                    // all fec packets have CONTAINS_VIDEO_DATA: https://github.com/LizardByte/Sunshine/blob/4ea331e541d57bb8c4827a717c708cdc1b34eadb/src/stream.cpp#L1440
                    flags: VideoHeaderFlags::CONTAINS_VIDEO_DATA,
                    extra_flags: VideoHeaderExtraFlags::empty(),
                    multi_fec_flags: 0x10,
                    multi_fec_blocks,
                    fec_info: VideoFecInfo {
                        data_shards_total: data_shards_count as u32,
                        shard_index: current_packet_count as u32,
                        fec_percentage: fec_percentage as u32,
                        unused: 0,
                    },
                };

                let packet = match build_packet(
                    &rtp_header,
                    &video_header,
                    parity_shard,
                    self.encryption.as_mut(),
                    &self.crypto_backend,
                    &mut self.tmp_buffer,
                ) {
                    Ok(packet) => packet,
                    Err(err) => {
                        warn!(?rtp_header, ?video_header, payload_len = parity_shard.len(), error = %err, "failed to build packet");
                        continue;
                    }
                };
                self.packet_queue.push_back(packet);

                current_sequence_number = current_sequence_number.wrapping_add(1);
                current_packet_count += 1;
            }
        }

        current_packet_count
    }

    pub fn poll_packet(&mut self) -> Option<Bytes> {
        self.packet_queue.pop_front()
    }
}

fn build_packet(
    rtp_header: &RtpVideoHeader,
    video_header: &VideoHeader,
    payload: &[u8],
    encrypted: Option<&mut Encryption>,
    crypto_backend: &dyn CryptoBackend,
    tmp_buffer: &mut [u8],
) -> Result<Bytes, Error> {
    let header_size = header_size(encrypted.is_some());
    let mut output = BytesMut::zeroed(header_size + payload.len());

    let buffer = if encrypted.is_some() {
        &mut *tmp_buffer
    } else {
        &mut output
    };

    #[allow(clippy::unwrap_used)]
    rtp_header.serialize(buffer[0..RtpVideoHeader::SIZE].as_mut_array().unwrap());
    #[allow(clippy::unwrap_used)]
    video_header.serialize(
        buffer[RtpVideoHeader::SIZE..(RtpVideoHeader::SIZE + VideoHeader::SIZE)]
            .as_mut_array()
            .unwrap(),
    );
    buffer[header_size..].copy_from_slice(payload);

    if let Some(encryption) = encrypted {
        let mut iv_array = [0; 12];
        iv_array[0..8].copy_from_slice(&encryption.iv_counter.to_le_bytes());
        iv_array[11] = b'V';

        let mut encrypted_header = EncryptedVideoHeader {
            iv: iv_array,
            frame_number: video_header.frame_index,
            tag: [0; 16],
        };

        crypto_backend.encrypt_aes_gcm(
            &encryption.aes_key,
            &iv_array,
            &tmp_buffer[0..(header_size + payload.len())],
            &mut output[EncryptedVideoHeader::SIZE..],
            &mut encrypted_header.iv,
        )?;

        encryption.iv_counter = encryption.iv_counter.wrapping_add(1);
    }

    Ok(output.freeze())
}
