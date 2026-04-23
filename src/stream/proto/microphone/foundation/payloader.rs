use std::{collections::VecDeque, time::Duration};

use thiserror::Error;

use crate::stream::{
    AesIv, AesKey,
    proto::{
        crypto::{CryptoBackend, CryptoError, round_to_pkcs7_safe_len},
        microphone::foundation::packet::{
            FOUNDATION_MAX_MIC_PACKET_SIZE, FOUNDATION_MIC_HEADER_FLAGS, FOUNDATION_MIC_IV_LEN,
            FOUNDATION_MIC_MAGIC, FOUNDATION_MIC_PACKET_TYPE_OPUS, FoundationMicHeader,
        },
    },
};

#[derive(Debug, Error)]
pub enum FoundationMicPayloaderError {
    #[error("the mic packet exceeded the maximum size of {FOUNDATION_MAX_MIC_PACKET_SIZE}")]
    PacketTooLarge,
    #[error("crypto: {0}")]
    Crypto(#[from] CryptoError),
}

#[derive(Debug)]
pub struct FoundationMicPayloaderConfig {
    pub encryption: Option<(AesKey, AesIv)>,
}

/// Foundation Extension
#[derive(Debug)]
pub struct FoundationMicPayloader<Crypto> {
    crypto_backend: Crypto,
    config: FoundationMicPayloaderConfig,
    current_packet: Option<Vec<u8>>,
    packets: VecDeque<Vec<u8>>,
    unused: Vec<Vec<u8>>,
    sequence_number: u16,
}

impl<Crypto> FoundationMicPayloader<Crypto>
where
    Crypto: CryptoBackend,
{
    pub fn new(config: FoundationMicPayloaderConfig, crypto_backend: Crypto) -> Self {
        Self {
            crypto_backend,
            config,
            current_packet: None,
            packets: Default::default(),
            unused: Default::default(),
            sequence_number: 0,
        }
    }

    pub fn push_frame(
        &mut self,
        timestamp: Duration,
        frame: &[u8],
    ) -> Result<(), FoundationMicPayloaderError> {
        let safe_len = if self.config.encryption.is_some() {
            FoundationMicHeader::SIZE + round_to_pkcs7_safe_len(frame.len())
        } else {
            FoundationMicHeader::SIZE + frame.len()
        };

        let mut packet = self.take_packet(safe_len);

        let header = FoundationMicHeader {
            flags: FOUNDATION_MIC_HEADER_FLAGS,
            packet_type: FOUNDATION_MIC_PACKET_TYPE_OPUS,
            sequence_number: self.sequence_number,
            timestamp: timestamp.as_millis() as u32,
            ssrc: FOUNDATION_MIC_MAGIC,
        };

        // This won't panic because the size is correct
        #[allow(clippy::unwrap_used)]
        header.serialize(packet[0..FoundationMicHeader::SIZE].as_mut_array().unwrap());

        if let Some((aes_key, aes_iv)) = self.config.encryption {
            // See
            // https://github.com/Yundi339/moonlight-common-c/blob/f59424a9f7ad86f2b6278a4e2b07fb2902d8b090/src/MicrophoneStream.c#L96-L116

            let mut iv = [0; FOUNDATION_MIC_IV_LEN];
            iv[0..4].copy_from_slice(
                &aes_iv
                    .wrapping_add(self.sequence_number as u32)
                    .to_be_bytes(),
            );

            let len = self.crypto_backend.encrypt_aes_cbc(
                &aes_key,
                &iv,
                frame,
                &mut packet[FoundationMicHeader::SIZE..],
            )?;
            packet.truncate(FoundationMicHeader::SIZE + len);

            // Check bounds
            if packet.len() > FOUNDATION_MAX_MIC_PACKET_SIZE {
                return Err(FoundationMicPayloaderError::PacketTooLarge);
            }
        } else {
            // Check bounds
            if packet.len() > FOUNDATION_MAX_MIC_PACKET_SIZE {
                return Err(FoundationMicPayloaderError::PacketTooLarge);
            }

            // just plaintext copy
            packet[FoundationMicHeader::SIZE..].copy_from_slice(frame);
        }

        self.sequence_number = self.sequence_number.wrapping_add(1);

        self.packets.push_back(packet);

        Ok(())
    }

    fn take_packet(&mut self, len: usize) -> Vec<u8> {
        self.unused
            .pop()
            .map(|mut x| {
                x.resize(len, 0);
                x
            })
            .unwrap_or_else(|| vec![0; len])
    }

    pub fn poll_packet(&mut self) -> Result<Option<&[u8]>, FoundationMicPayloaderError> {
        if let Some(old_packet) = self.current_packet.take() {
            self.unused.push(old_packet);
        }

        if let Some(packet) = self.packets.pop_front() {
            self.current_packet = Some(packet);

            // The value was just set to some, this cannot fail
            #[allow(clippy::unwrap_used)]
            let packet = self.current_packet.as_ref().unwrap();
            Ok(Some(packet))
        } else {
            Ok(None)
        }
    }
}
