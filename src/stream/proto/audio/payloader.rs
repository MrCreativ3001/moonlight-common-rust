use std::{array, collections::VecDeque};

use bytes::{Bytes, BytesMut};
use fec_rs::ReedSolomon;
use thiserror::Error;

use crate::{
    error::Error,
    stream::{
        AesIv, AesKey,
        proto::{
            DynCryptoBackend,
            audio::{
                create_audio_reed_solomon,
                packet::{
                    AudioFecHeader, RTP_AUDIO_HEADER, RTP_PAYLOAD_TYPE_AUDIO,
                    RTP_PAYLOAD_TYPE_AUDIO_FEC, RtpAudioHeader,
                },
            },
            crypto::{CryptoBackend, round_to_pkcs7_safe_len},
        },
    },
};

#[cfg(test)]
#[path = "./payloader_tests.rs"]
mod payloader_tests;

pub struct AudioPayloaderConfig {
    pub fec: bool,
    /// The size of one opus frame in bytes
    pub frame_len: usize,
    pub encryption: Option<(AesKey, AesIv)>,
}

#[derive(Debug, Error)]
pub enum AudioPayloaderError {
    /// This frame is bigger than allowed
    #[error("opus frame has invalid size")]
    InvalidFrameSize,
}

impl From<AudioPayloaderError> for Error {
    fn from(value: AudioPayloaderError) -> Self {
        Self::Other(value.into())
    }
}

pub struct AudioPayloader {
    // config options
    #[allow(unused)]
    crypto_backend: DynCryptoBackend,
    reed_solomon: Option<ReedSolomon>,
    frame_len: usize,
    encryption: Option<(AesKey, AesIv)>,
    // payloading
    base_timestamp: u32,
    sequence_number: u16,
    data_shards: [Vec<u8>; 4],
    packet_queue: VecDeque<Bytes>,
}

impl AudioPayloader {
    pub fn new(config: AudioPayloaderConfig, crypto_backend: DynCryptoBackend) -> Self {
        debug_assert!(config.frame_len > 0);

        Self {
            crypto_backend,
            reed_solomon: config.fec.then(create_audio_reed_solomon),
            encryption: config.encryption,
            frame_len: config.frame_len,
            data_shards: array::from_fn(|_| vec![0; config.frame_len]),
            sequence_number: 0,
            base_timestamp: 0,
            packet_queue: Default::default(),
        }
    }

    /// Pushes one opus frame to the payloader.
    pub fn push_frame(&mut self, timestamp: u32, frame: &[u8]) -> Result<(), Error> {
        if frame.len() != self.frame_len {
            return Err(AudioPayloaderError::InvalidFrameSize.into());
        }

        let safe_packet_size = self.safe_packet_size();
        let mut packet = BytesMut::zeroed(safe_packet_size);

        let rtp_header = RtpAudioHeader {
            header: RTP_AUDIO_HEADER,
            packet_type: RTP_PAYLOAD_TYPE_AUDIO,
            sequence_number: self.sequence_number,
            timestamp,
            ssrc: 0,
        };

        rtp_header.serialize(
            packet[0..RtpAudioHeader::SIZE]
                .as_mut_array()
                .expect("valid slice size"),
        );

        let payload = &mut packet[RtpAudioHeader::SIZE..];
        let payload_len = if let Some((aes_key, aes_iv)) = self.encryption {
            let mut iv = [0u8; 16];
            iv[0..4].copy_from_slice(
                &aes_iv
                    .wrapping_add(self.sequence_number as u32)
                    .to_be_bytes(),
            );

            self.crypto_backend
                .encrypt_aes_cbc(&aes_key, &iv, frame, payload)?
        } else {
            payload[0..self.frame_len].copy_from_slice(frame);
            frame.len()
        };

        // Get shard index, Increment sequence number
        let shard_index = (self.sequence_number % 4) as usize;
        self.sequence_number = self.sequence_number.wrapping_add(1);

        // Set base timestamp for future fec packets
        if shard_index == 0 {
            self.base_timestamp = timestamp;
        }

        // Needed for fec generation: put all shards into their temporary buffer
        self.data_shards[shard_index].resize(payload_len, 0);
        self.data_shards[shard_index].copy_from_slice(&payload[0..payload_len]);

        // Insert packet into queue, truncate before
        packet.truncate(RtpAudioHeader::SIZE + payload_len);
        self.packet_queue.push_back(packet.freeze());

        // Generate fec packets if necessary
        if shard_index == 3
            && let Some(reed_solomon) = self.reed_solomon.as_ref()
        {
            let mut fec_packet1 = BytesMut::zeroed(safe_packet_size);
            let mut fec_packet2 = BytesMut::zeroed(safe_packet_size);

            // Generate Audio Headers
            let rtp_header1 = RtpAudioHeader {
                header: RTP_AUDIO_HEADER,
                packet_type: RTP_PAYLOAD_TYPE_AUDIO_FEC,
                sequence_number: self.sequence_number,
                timestamp: 0,
                ssrc: 0,
            };
            let rtp_header2 = RtpAudioHeader {
                header: RTP_AUDIO_HEADER,
                packet_type: RTP_PAYLOAD_TYPE_AUDIO_FEC,
                sequence_number: self.sequence_number.wrapping_add(1),
                timestamp: 0,
                ssrc: 0,
            };
            // Info: Fec Packets don't increment the actual sequence number for audio packets

            rtp_header1.serialize(
                fec_packet1[0..RtpAudioHeader::SIZE]
                    .as_mut_array()
                    .expect("valid slice size"),
            );
            rtp_header2.serialize(
                fec_packet2[0..RtpAudioHeader::SIZE]
                    .as_mut_array()
                    .expect("valid slice size"),
            );

            // Generate Fec Headers
            let base_sequence_number = self.sequence_number.wrapping_sub(4);
            let fec_header1 = AudioFecHeader {
                fec_shard_index: 0,
                payload_type: RTP_PAYLOAD_TYPE_AUDIO,
                base_sequence_number,
                base_timestamp: self.base_timestamp,
                ssrc: 0,
            };
            let fec_header2 = AudioFecHeader {
                fec_shard_index: 1,
                payload_type: RTP_PAYLOAD_TYPE_AUDIO,
                base_sequence_number,
                base_timestamp: self.base_timestamp,
                ssrc: 0,
            };

            fec_header1.serialize(
                fec_packet1[RtpAudioHeader::SIZE..(RtpAudioHeader::SIZE + AudioFecHeader::SIZE)]
                    .as_mut_array()
                    .expect("valid slice size"),
            );
            fec_header2.serialize(
                fec_packet2[RtpAudioHeader::SIZE..(RtpAudioHeader::SIZE + AudioFecHeader::SIZE)]
                    .as_mut_array()
                    .expect("valid slice size"),
            );

            // Generate Fec Payload
            reed_solomon
                .encode_sep(
                    &self.data_shards,
                    &mut [
                        &mut fec_packet1[(RtpAudioHeader::SIZE + AudioFecHeader::SIZE)..],
                        &mut fec_packet2[(RtpAudioHeader::SIZE + AudioFecHeader::SIZE)..],
                    ],
                )
                .expect("encode audio fec packets using reed solomon");

            // Insert fec packets into queue
            self.packet_queue.push_back(fec_packet1.freeze());
            self.packet_queue.push_back(fec_packet2.freeze());
        }

        Ok(())
    }

    pub fn poll_packet(&mut self) -> Option<Bytes> {
        self.packet_queue.pop_front()
    }

    fn safe_packet_size(&self) -> usize {
        let payload_len = if self.encryption.is_some() {
            round_to_pkcs7_safe_len(self.frame_len)
        } else {
            self.frame_len
        };

        RtpAudioHeader::SIZE + AudioFecHeader::SIZE + payload_len
    }

    #[cfg(test)]
    pub(crate) fn set_sequence_number(&mut self, sequence_number: u16) {
        self.sequence_number = sequence_number;
    }
}
