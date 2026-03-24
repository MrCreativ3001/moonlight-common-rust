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
        // Tag is assigned in encrypt
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
#[instrument(level = Level::TRACE, skip(crypto_backend, encrypted_message, message))]
pub fn decrypt_client_rtsp_message_into<Crypto>(
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
    trace!(header = ?header, "parsed encryption header");

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

    iv[10] = b'C'; // Client
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

pub fn encrypt_server_rtsp_message_into<Crypto>(
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

    iv[10] = b'H'; // Host
    iv[11] = b'R'; // RTSP stream

    let mut header = RtspEncryptionHeader {
        encrypted: true,
        len: message.len(),
        sequence_number,
        // Tag is assigned in encrypt
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
    trace!(header = ?header, "parsed encryption header");

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

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod test {
    use crate::stream::{
        AesKey,
        proto::{
            crypto::CryptoBackend,
            rtsp::encryption::{
                decrypt_client_rtsp_message_into, decrypt_server_rtsp_message_into,
                encrypt_client_rtsp_message_into, encrypt_server_rtsp_message_into,
            },
        },
    };
    use std::error::Error;

    fn test_clientbound_message_encryption<Crypto>(
        crypto: &Crypto,
        aes_key: AesKey,
        sequence_number: usize,
        expected_decrypted_message: &[u8],
        expected_encrypted_message: &[u8],
    ) where
        Crypto: CryptoBackend + 'static,
        Crypto::Error: Error + 'static,
    {
        let mut encrypted_message = vec![0; expected_encrypted_message.len()];
        let encrypted_message_len = encrypt_client_rtsp_message_into(
            crypto,
            aes_key,
            sequence_number,
            expected_decrypted_message,
            &mut encrypted_message,
        )
        .unwrap();
        assert_eq!(
            &encrypted_message[0..encrypted_message_len],
            expected_encrypted_message
        );

        let mut decrypted_message = vec![0; expected_decrypted_message.len()];
        let decrypted_message_len = decrypt_client_rtsp_message_into(
            crypto,
            aes_key,
            sequence_number,
            expected_encrypted_message,
            &mut decrypted_message,
        )
        .unwrap();

        assert_eq!(
            &decrypted_message[0..decrypted_message_len],
            expected_decrypted_message
        );
    }

    fn test_serverbound_message_encryption<Crypto>(
        crypto: &Crypto,
        aes_key: AesKey,
        sequence_number: usize,
        expected_decrypted_message: &[u8],
        expected_encrypted_message: &[u8],
    ) where
        Crypto: CryptoBackend + 'static,
        Crypto::Error: Error + 'static,
    {
        let mut encrypted_message = vec![0; expected_encrypted_message.len()];
        let encrypted_message_len = encrypt_server_rtsp_message_into(
            crypto,
            aes_key,
            sequence_number,
            expected_decrypted_message,
            &mut encrypted_message,
        )
        .unwrap();
        assert_eq!(
            &encrypted_message[0..encrypted_message_len],
            expected_encrypted_message
        );

        let mut decrypted_message = vec![0; expected_decrypted_message.len()];
        let decrypted_message_len = decrypt_server_rtsp_message_into(
            crypto,
            aes_key,
            sequence_number,
            expected_encrypted_message,
            &mut decrypted_message,
        )
        .unwrap();

        assert_eq!(
            &decrypted_message[0..decrypted_message_len],
            expected_decrypted_message
        );
    }

    fn test_crypto<Crypto>(crypto: &Crypto)
    where
        Crypto: CryptoBackend + 'static,
        Crypto::Error: Error + 'static,
    {
        test_clientbound_message_encryption(
            crypto,
            AesKey([10, 43, 54, 34, 65, 47, 90, 201, 2, 3, 6, 16, 6, 5, 2, 23]),
            3,
            // a describe request
            &[
                68, 69, 83, 67, 82, 73, 66, 69, 32, 114, 116, 115, 112, 101, 110, 99, 58, 47, 47,
                49, 57, 50, 46, 49, 54, 56, 46, 49, 55, 56, 46, 49, 52, 48, 58, 52, 56, 48, 49, 48,
                32, 82, 84, 83, 80, 47, 49, 46, 48, 13, 10, 65, 99, 99, 101, 112, 116, 58, 32, 97,
                112, 112, 108, 105, 99, 97, 116, 105, 111, 110, 47, 115, 100, 112, 13, 10, 73, 102,
                45, 77, 111, 100, 105, 102, 105, 101, 100, 45, 83, 105, 110, 99, 101, 58, 32, 84,
                104, 117, 44, 32, 48, 49, 32, 74, 97, 110, 32, 49, 57, 55, 48, 32, 48, 48, 58, 48,
                48, 58, 48, 48, 32, 71, 77, 84, 13, 10, 67, 83, 101, 113, 58, 32, 50, 13, 10, 88,
                45, 71, 83, 45, 67, 108, 105, 101, 110, 116, 86, 101, 114, 115, 105, 111, 110, 58,
                32, 49, 52, 13, 10, 72, 111, 115, 116, 58, 32, 49, 57, 50, 46, 49, 54, 56, 46, 49,
                55, 56, 46, 49, 52, 48, 58, 52, 56, 48, 49, 48, 13, 10, 13, 10,
            ],
            &[
                128, 0, 0, 190, 0, 0, 0, 3, 138, 240, 126, 108, 167, 85, 198, 202, 248, 79, 2, 3,
                132, 111, 89, 123, 6, 216, 67, 36, 193, 146, 72, 94, 224, 190, 17, 23, 176, 10,
                232, 193, 206, 4, 190, 29, 105, 191, 26, 151, 175, 71, 219, 250, 91, 5, 151, 73,
                59, 109, 39, 195, 56, 132, 210, 53, 174, 211, 37, 240, 181, 185, 237, 186, 96, 10,
                177, 61, 4, 137, 25, 112, 134, 84, 137, 24, 138, 168, 238, 199, 234, 94, 214, 239,
                164, 132, 57, 203, 233, 97, 196, 44, 227, 195, 147, 49, 42, 71, 2, 197, 32, 11,
                204, 127, 167, 134, 36, 171, 125, 12, 187, 76, 126, 35, 186, 223, 205, 186, 205,
                125, 196, 8, 242, 129, 189, 20, 47, 27, 28, 179, 85, 54, 115, 88, 250, 183, 34,
                132, 58, 186, 217, 220, 159, 74, 41, 231, 236, 252, 23, 243, 16, 45, 154, 102, 145,
                35, 228, 14, 26, 149, 0, 13, 102, 112, 46, 158, 132, 162, 36, 88, 189, 18, 251,
                120, 138, 20, 60, 4, 254, 193, 208, 1, 126, 151, 228, 221, 111, 250, 208, 10, 88,
                116, 124, 253, 122, 188, 178, 137, 252, 231, 160, 185, 165, 214, 228, 15,
            ],
        );

        test_serverbound_message_encryption(
            crypto,
            AesKey([
                123, 84, 84, 244, 34, 48, 184, 184, 244, 84, 93, 57, 58, 46, 233, 48,
            ]),
            2,
            "RTSP/1.0 200 OK\r\nCSeq: 1\r\n\r\n".as_bytes(),
            &[
                128, 0, 0, 28, 0, 0, 0, 2, 74, 189, 106, 107, 29, 189, 223, 220, 99, 137, 11, 117,
                185, 14, 240, 117, 21, 158, 139, 47, 77, 168, 35, 47, 104, 82, 85, 183, 6, 183,
                178, 105, 147, 230, 106, 68, 214, 92, 123, 45, 123, 226, 98, 235,
            ],
        );
    }

    #[cfg(feature = "openssl")]
    #[test]
    fn openssl() {
        use crate::crypto::openssl::OpenSSLCryptoBackend;

        test_crypto(&OpenSSLCryptoBackend);
    }

    #[cfg(feature = "rustcrypto")]
    #[test]
    fn rustcrypto() {
        // test_crypto(RustCryptoBackend);
        todo!()
    }
}
