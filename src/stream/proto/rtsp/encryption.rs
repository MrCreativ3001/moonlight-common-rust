use thiserror::Error;

use tracing::{Level, debug, instrument, trace};

use std::error::Error;

use crate::stream::{
    AesKey,
    proto::{
        crypto::{CipherAlgorithm, CryptoBackend},
        rtsp::packet::RtspEncryptionHeader,
    },
};

#[derive(Debug, Error)]
pub enum RtspEncryptionError {
    #[error("message is smaller than the required rtsp encryption header")]
    MessageTooSmallHeader,
    #[error("the received message is unencrypted!")]
    MessageUnencrypted,
    #[error("the expected message length doesn't match the content length of the message")]
    EncryptedMessageWrongSize,
    #[error("the provided output buffer is too small")]
    OutputTooSmall,
    #[error("crypto: {0}")]
    Crypto(Box<dyn Error>),
}

/// References:
/// - https://github.com/moonlight-stream/moonlight-common-c/blob/b126e481a195fdc7152d211def17190e3434bcce/src/RtspConnection.c#L100
pub fn encrypt_client_rtsp_message_into<Crypto>(
    crypto_backend: &Crypto,
    aes_key: AesKey,
    sequence_number: usize,
    message: &[u8],
    encrypted_message: &mut [u8],
) -> Result<usize, RtspEncryptionError>
where
    Crypto: CryptoBackend,
    Crypto::Error: Error + 'static,
{
    let len = RtspEncryptionHeader::SIZE + message.len();
    if encrypted_message.len() < len {
        return Err(RtspEncryptionError::OutputTooSmall);
    }

    let mut iv = [0; 12];
    iv[0..4].copy_from_slice(&u32::to_le_bytes(sequence_number as u32));

    iv[10] = b'C'; // Client originated
    iv[11] = b'R'; // RTSP stream

    let mut header = RtspEncryptionHeader {
        encrypted: true,
        len: message.len(),
        sequence_number,
        tag: [0; 16],
    };

    crypto_backend
        .encrypt(
            CipherAlgorithm::Aes128Gcm,
            &aes_key,
            &iv,
            &mut header.tag,
            message,
            &mut encrypted_message[RtspEncryptionHeader::SIZE..],
        )
        .map_err(|err| RtspEncryptionError::Crypto(Box::new(err)))?;

    #[allow(clippy::unwrap_used)]
    // This won't panic because we're literally using the size to get the slice
    header.serialize(
        encrypted_message[0..RtspEncryptionHeader::SIZE]
            .as_mut_array::<{ RtspEncryptionHeader::SIZE }>()
            .unwrap(),
    );

    Ok(len)
}

// TODO
/// References:
/// - Sunshine:
///   - https://github.com/LizardByte/Sunshine/blob/24b66feddaf6df889dc1330a02b3289c09ec62cc/src/rtsp.cpp#L176-L239
///   - https://github.com/LizardByte/Sunshine/blob/24b66feddaf6df889dc1330a02b3289c09ec62cc/src/rtsp.cpp#L127-L174
pub fn decrypt_client_rtsp_message_into() {
    todo!()
}

pub fn encrypt_server_rtsp_message_into() {
    todo!()
}

/// References:
/// - https://github.com/moonlight-stream/moonlight-common-c/blob/b126e481a195fdc7152d211def17190e3434bcce/src/RtspConnection.c#L166-L224
#[instrument(level = Level::TRACE, skip(crypto_backend, encrypted_message, message))]
pub fn decrypt_server_rtsp_message_into<Crypto>(
    crypto_backend: &Crypto,
    aes_key: AesKey,
    sequence_number: usize,
    encrypted_message: &[u8],
    message: &mut [u8],
) -> Result<usize, RtspEncryptionError>
where
    Crypto: CryptoBackend,
    Crypto::Error: Error + 'static,
{
    if encrypted_message.len() < RtspEncryptionHeader::SIZE {
        return Err(RtspEncryptionError::MessageTooSmallHeader);
    }

    // We checked that the size must match
    #[allow(clippy::unwrap_used)]
    let header = RtspEncryptionHeader::deserialize(
        encrypted_message[0..RtspEncryptionHeader::SIZE]
            .try_into()
            .unwrap(),
    );
    trace!(header = ?header, "parsed header");

    if !header.encrypted {
        return Err(RtspEncryptionError::MessageUnencrypted);
    }

    if encrypted_message.len() != RtspEncryptionHeader::SIZE + header.len {
        debug!(header = ?header, expected_len = RtspEncryptionHeader::SIZE + header.len, got_len = encrypted_message.len(), "encrypted rtsp message doesn't match expected size");
        return Err(RtspEncryptionError::EncryptedMessageWrongSize);
    }
    let message_len = header.len;
    let ciphertext =
        &encrypted_message[RtspEncryptionHeader::SIZE..RtspEncryptionHeader::SIZE + header.len];

    let mut iv = [0; 12];
    iv[0..4].copy_from_slice(&u32::to_le_bytes(sequence_number as u32));

    iv[10] = b'H'; // Host
    iv[11] = b'R'; // RTSP

    if message.len() < ciphertext.len() {
        return Err(RtspEncryptionError::OutputTooSmall);
    }

    crypto_backend
        .decrypt(
            CipherAlgorithm::Aes128Gcm,
            &aes_key,
            &iv,
            Some(&header.tag),
            ciphertext,
            &mut message[..message_len],
        )
        .map_err(|err| RtspEncryptionError::Crypto(Box::new(err)))?;

    Ok(message_len)
}

mod test {
    use crate::stream::proto::crypto::CryptoBackend;

    fn test_clientbound_message_encryption(
        crypto: impl CryptoBackend,
        decrypted_message: &[u8],
        encrypted_message: &[u8],
    ) {
        todo!()
    }

    fn test_serverbound_message_encryption(
        crypto: impl CryptoBackend,
        decrypted_message: &[u8],
        encrypted_message: &[u8],
    ) {
        todo!()
    }

    fn test_crypto(crypto: impl CryptoBackend) {
        todo!()
    }

    #[cfg(feature = "openssl")]
    #[test]
    fn openssl() {
        use crate::crypto::openssl::OpenSSLCryptoBackend;

        test_crypto(OpenSSLCryptoBackend);
    }

    #[cfg(feature = "rustcrypto")]
    #[test]
    fn rustcrypto() {
        // test_crypto(RustCryptoBackend);
        todo!()
    }
}
