use std::{array, collections::VecDeque};

use fec_rs::ReedSolomon;
use thiserror::Error;

use crate::{
    crypto::disabled::DisabledCryptoBackend,
    stream::{
        AesIv, AesKey,
        proto::{
            audio::{
                create_audio_reed_solomon,
                packet::{
                    AudioFecHeader, RTP_AUDIO_HEADER, RTP_PAYLOAD_TYPE_AUDIO,
                    RTP_PAYLOAD_TYPE_AUDIO_FEC, RtpAudioHeader,
                },
            },
            crypto::{CryptoBackend, CryptoError, round_to_pkcs7_safe_len},
        },
    },
};

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
    #[error("crypto: {0}")]
    Crypto(#[from] CryptoError),
}

pub struct AudioPayloader<Crypto> {
    // config options
    #[allow(unused)]
    crypto_backend: Crypto,
    reed_solomon: Option<ReedSolomon>,
    frame_len: usize,
    encryption: Option<(AesKey, AesIv)>,
    // payloading
    base_timestamp: u32,
    sequence_number: u16,
    data_shards: [Vec<u8>; 4],
    packet_queue_used_front: bool,
    packet_queue: VecDeque<Vec<u8>>,
    unused: Vec<Vec<u8>>,
}

impl AudioPayloader<DisabledCryptoBackend> {
    pub fn new_unencrypted(config: AudioPayloaderConfig) -> Self {
        assert_eq!(
            config.encryption, None,
            "Cannot have unencrypted audio payloader with aes key and iv"
        );

        Self::new(config, DisabledCryptoBackend)
    }
}

impl<Crypto> AudioPayloader<Crypto>
where
    Crypto: CryptoBackend,
{
    pub fn new(config: AudioPayloaderConfig, crypto_backend: Crypto) -> Self {
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
            packet_queue_used_front: Default::default(),
            unused: Vec::new(),
        }
    }

    /// Pushes one opus frame to the payloader.
    pub fn push_frame(&mut self, timestamp: u32, frame: &[u8]) -> Result<(), AudioPayloaderError> {
        if frame.len() != self.frame_len {
            return Err(AudioPayloaderError::InvalidFrameSize);
        }

        let mut packet = self.dequeue_packet()?;

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

        // This is an edge case
        if self.packet_queue.is_empty() {
            self.packet_queue_used_front = false;
        }

        // Insert packet into queue, truncate before
        packet.truncate(RtpAudioHeader::SIZE + payload_len);
        self.packet_queue.push_back(packet);

        // Generate fec packets if necessary
        if self.reed_solomon.is_some() && shard_index == 3 {
            let mut fec_packet1 = self.dequeue_packet()?;
            let mut fec_packet2 = self.dequeue_packet()?;

            let reed_solomon = self.reed_solomon.as_ref().expect("reed solomon encoder");

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
            self.packet_queue.push_back(fec_packet1);
            self.packet_queue.push_back(fec_packet2);
        }

        Ok(())
    }

    pub fn poll_packet(&mut self) -> Result<Option<&[u8]>, AudioPayloaderError> {
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

    fn safe_packet_size(&self) -> usize {
        let payload_len = if self.encryption.is_some() {
            round_to_pkcs7_safe_len(self.frame_len)
        } else {
            self.frame_len
        };

        RtpAudioHeader::SIZE + AudioFecHeader::SIZE + payload_len
    }

    fn dequeue_packet(&mut self) -> Result<Vec<u8>, AudioPayloaderError> {
        if let Some(mut vec) = self.unused.pop() {
            vec.resize(self.safe_packet_size(), 0);
            return Ok(vec);
        }

        Ok(vec![0; self.safe_packet_size()])
    }

    #[cfg(test)]
    pub(crate) fn set_sequence_number(&mut self, sequence_number: u16) {
        self.sequence_number = sequence_number;
    }
}
