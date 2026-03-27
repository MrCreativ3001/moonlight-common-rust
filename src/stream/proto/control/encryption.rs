use std::error::Error;

use thiserror::Error;
use tracing::warn;

use crate::stream::{
    AesKey,
    proto::{
        control::{
            ControlEncryptionMethod,
            packet::{
                ENCRYPTED_CONTROL_PACKET_AES_GCM_TAG_LENGTH, ENCRYPTED_CONTROL_PACKET_TYPE,
                EncryptedControlHeader,
            },
        },
        crypto::{CipherAlgorithm, CryptoBackend},
    },
};

#[derive(Debug, Error)]
pub enum ControlEncryptionError {
    #[error("packet is too small")]
    PacketTooSmall,
    #[error("invalid encryption packet type")]
    InvalidEncryptionHeaderPacketType,
    #[error("the encryption header len doesn't match the packet length")]
    EncryptionHeaderLengthMismatch,
    #[error("payload is too small")]
    PayloadTooSmall,
    #[error("payload is too small")]
    EncryptionHeaderLengthTooSmall,
    #[error("crypto: {0}")]
    Crypto(Box<dyn Error>),
}

fn encrypt_control_packet_into<Crypto>(
    crypto: &Crypto,
    aes_key: AesKey,
    sequence_number: u32,
    iv: &[u8],
    unencrypted_packet: &[u8],
    encrypted_packet: &mut [u8],
) -> Result<usize, ControlEncryptionError>
where
    Crypto: CryptoBackend,
    Crypto::Error: Error + 'static,
{
    let mut header = EncryptedControlHeader {
        ty: ENCRYPTED_CONTROL_PACKET_TYPE,
        sequence_number,
        len: EncryptedControlHeader::len_with_payload_size(unencrypted_packet.len()) as u16,
        tag: [0; _],
    };

    crypto
        .encrypt(
            CipherAlgorithm::Aes128Gcm,
            &aes_key,
            iv,
            &mut header.tag,
            unencrypted_packet,
            &mut encrypted_packet[EncryptedControlHeader::SIZE..],
        )
        .map_err(|err| ControlEncryptionError::Crypto(err.into()))?;

    // Only serialize header after having encrypted to also serialize the tag
    // The size matches and the caller is responsible for the list being big enough
    #[allow(clippy::unwrap_used)]
    header.serialize(
        encrypted_packet[0..EncryptedControlHeader::SIZE]
            .as_mut_array()
            .unwrap(),
    );

    Ok(EncryptedControlHeader::SIZE + unencrypted_packet.len())
}

fn decrypt_control_packet_into<Crypto>(
    crypto: &Crypto,
    aes_key: AesKey,
    // u32 = sequence_number
    generate_iv: impl FnOnce(u32, &mut [u8; 16]) -> usize,
    encrypted_packet: &[u8],
    unencrypted_packet: &mut [u8],
) -> Result<usize, ControlEncryptionError>
where
    Crypto: CryptoBackend,
    Crypto::Error: Error + 'static,
{
    if encrypted_packet.len() < EncryptedControlHeader::SIZE {
        warn!(packet = ?encrypted_packet, required_len = ?EncryptedControlHeader::SIZE, "dropping packet that is smaller than the encrypted control header");
        return Err(ControlEncryptionError::PacketTooSmall);
    }

    // This is allowed because the size was checked
    #[allow(clippy::unwrap_used)]
    let encrypted_header = EncryptedControlHeader::deserialize(
        encrypted_packet[0..EncryptedControlHeader::SIZE]
            .try_into()
            .unwrap(),
    );

    if encrypted_header.ty != ENCRYPTED_CONTROL_PACKET_TYPE {
        warn!(encrypted_encrypted_header = ?encrypted_header, got_ty = encrypted_header.ty, expected_ty = ENCRYPTED_CONTROL_PACKET_TYPE, "dropping packet because of invalid packet type, expected encrypted header");
        return Err(ControlEncryptionError::InvalidEncryptionHeaderPacketType);
    }

    // 4 = sizeof(sequence_number) in EncryptedControlHeader
    const SUB_LEN: usize = 4 + ENCRYPTED_CONTROL_PACKET_AES_GCM_TAG_LENGTH;
    if encrypted_header.len < SUB_LEN as u16 {
        warn!(encrypted_header = ?encrypted_header, got_len = encrypted_header.len, required_len = SUB_LEN, "dropping packet because of invalid encryption header length");
        return Err(ControlEncryptionError::InvalidEncryptionHeaderPacketType);
    }

    let encrypted_payload = &encrypted_packet[EncryptedControlHeader::SIZE..];
    let expected_encrypted_payload_len = encrypted_header
        .payload_size()
        .ok_or(ControlEncryptionError::EncryptionHeaderLengthTooSmall)?;

    if encrypted_payload.len() != expected_encrypted_payload_len as usize {
        return Err(ControlEncryptionError::EncryptionHeaderLengthMismatch);
    }

    let mut iv = [0; _];
    let iv_len = generate_iv(encrypted_header.sequence_number, &mut iv);

    let len = crypto
        .decrypt(
            CipherAlgorithm::Aes128Gcm,
            &aes_key,
            &iv[0..iv_len],
            Some(&encrypted_header.tag),
            encrypted_payload,
            unencrypted_packet,
        )
        .map_err(|err| ControlEncryptionError::Crypto(err.into()))?;

    Ok(len)
}

/// The caller must ensure that encrypted_packet is big enough.
pub fn encrypt_server_control_packet_into<Crypto>(
    crypto: &Crypto,
    encryption_method: ControlEncryptionMethod,
    aes_key: AesKey,
    sequence_number: u32,
    unencrypted_packet: &[u8],
    encrypted_packet: &mut [u8],
) -> Result<usize, ControlEncryptionError>
where
    Crypto: CryptoBackend,
    Crypto::Error: Error + 'static,
{
    let mut iv = [0; _];
    let iv_len = generate_iv(encryption_method, false, sequence_number, &mut iv);

    encrypt_control_packet_into(
        crypto,
        aes_key,
        sequence_number,
        &iv[0..iv_len],
        unencrypted_packet,
        encrypted_packet,
    )
}

/// The encrypted_payload is the part after the header.
///
/// References:
/// - https://github.com/moonlight-stream/moonlight-common-c/blob/62687809b1f7410c3db4be2527503a54ae408d70/src/ControlStream.c#L1219-L1253
/// - https://github.com/moonlight-stream/moonlight-common-c/blob/62687809b1f7410c3db4be2527503a54ae408d70/src/ControlStream.c#L593-L663
pub fn decrypt_server_control_packet_into<Crypto>(
    crypto: &Crypto,
    encryption_method: ControlEncryptionMethod,
    aes_key: AesKey,
    encrypted_packet: &[u8],
    unencrypted_packet: &mut [u8],
) -> Result<usize, ControlEncryptionError>
where
    Crypto: CryptoBackend,
    Crypto::Error: Error + 'static,
{
    decrypt_control_packet_into(
        crypto,
        aes_key,
        |sequence_number, iv| generate_iv(encryption_method, false, sequence_number, iv),
        encrypted_packet,
        unencrypted_packet,
    )
}

/// The caller must ensure that encrypted_packet is big enough.
///
/// References:
/// - https://github.com/moonlight-stream/moonlight-common-c/blob/62687809b1f7410c3db4be2527503a54ae408d70/src/ControlStream.c#L703-L740
/// - https://github.com/moonlight-stream/moonlight-common-c/blob/62687809b1f7410c3db4be2527503a54ae408d70/src/ControlStream.c#L548-L591
pub fn encrypt_client_control_packet_into<Crypto>(
    crypto: &Crypto,
    encryption_method: ControlEncryptionMethod,
    aes_key: AesKey,
    sequence_number: u32,
    unencrypted_packet: &[u8],
    encrypted_packet: &mut [u8],
) -> Result<usize, ControlEncryptionError>
where
    Crypto: CryptoBackend,
    Crypto::Error: Error + 'static,
{
    let mut iv = [0; _];
    let iv_len = generate_iv(encryption_method, true, sequence_number, &mut iv);

    encrypt_control_packet_into(
        crypto,
        aes_key,
        sequence_number,
        &iv[0..iv_len],
        unencrypted_packet,
        encrypted_packet,
    )
}

/// The encrypted_payload is the part after the header.
///
/// References:
/// - https://github.com/moonlight-stream/moonlight-common-c/blob/62687809b1f7410c3db4be2527503a54ae408d70/src/ControlStream.c#L1219-L1253
/// - https://github.com/moonlight-stream/moonlight-common-c/blob/62687809b1f7410c3db4be2527503a54ae408d70/src/ControlStream.c#L593-L663
pub fn decrypt_client_control_packet_into<Crypto>(
    crypto: &Crypto,
    encryption_method: ControlEncryptionMethod,
    aes_key: AesKey,
    encrypted_packet: &[u8],
    unencrypted_packet: &mut [u8],
) -> Result<usize, ControlEncryptionError>
where
    Crypto: CryptoBackend,
    Crypto::Error: Error + 'static,
{
    decrypt_control_packet_into(
        crypto,
        aes_key,
        |sequence_number, iv| generate_iv(encryption_method, true, sequence_number, iv),
        encrypted_packet,
        unencrypted_packet,
    )
}

/// Returns the iv length
///
/// References:
/// - Clientoriginated iv: https://github.com/moonlight-stream/moonlight-common-c/blob/62687809b1f7410c3db4be2527503a54ae408d70/src/ControlStream.c#L617-L637
fn generate_iv(
    encryption: ControlEncryptionMethod,
    is_clientoriginated: bool,
    sequence_number: u32,
    iv: &mut [u8; 16],
) -> usize {
    match encryption {
        ControlEncryptionMethod::Nvidia => {
            iv[0] = sequence_number as u8;

            16
        }
        ControlEncryptionMethod::Sunshine => {
            iv[0..4].copy_from_slice(&sequence_number.to_le_bytes());

            if is_clientoriginated {
                iv[10] = b'C';
            } else {
                iv[10] = b'H';
            }
            iv[11] = b'C';

            12
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod test {
    use crate::stream::{
        AesIv, AesKey,
        proto::{
            control::{
                ControlEncryptionMethod,
                encryption::{
                    decrypt_client_control_packet_into, decrypt_server_control_packet_into,
                    encrypt_client_control_packet_into, encrypt_server_control_packet_into,
                    generate_iv,
                },
            },
            crypto::CryptoBackend,
        },
    };
    use std::error::Error;

    fn test_iv_result(
        method: ControlEncryptionMethod,
        is_clientoriginated: bool,
        sequence_number: u32,
        expected_iv: &[u8],
    ) {
        let mut iv = [0; _];
        let len = generate_iv(method, is_clientoriginated, sequence_number, &mut iv);
        assert_eq!(&iv[0..len], expected_iv);
    }

    #[test]
    fn iv() {
        // Nvidia
        test_iv_result(
            ControlEncryptionMethod::Nvidia,
            false,
            0,
            &[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        );
        test_iv_result(
            ControlEncryptionMethod::Nvidia,
            false,
            1,
            &[1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        );

        test_iv_result(
            ControlEncryptionMethod::Nvidia,
            true,
            0,
            &[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        );
        test_iv_result(
            ControlEncryptionMethod::Nvidia,
            true,
            1,
            &[1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        );

        // Sunshine
        test_iv_result(
            ControlEncryptionMethod::Sunshine,
            false,
            0,
            &[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 72, 67],
        );
        test_iv_result(
            ControlEncryptionMethod::Sunshine,
            false,
            1,
            &[1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 72, 67],
        );

        test_iv_result(
            ControlEncryptionMethod::Sunshine,
            true,
            0,
            &[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 67, 67],
        );
        test_iv_result(
            ControlEncryptionMethod::Sunshine,
            true,
            1,
            &[1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 67, 67],
        );
    }

    fn test_clientbound_packet<Crypto>(crypto: Crypto)
    where
        Crypto: CryptoBackend,
        Crypto::Error: Error + 'static,
    {
        let aes_key = AesKey([
            198, 75, 90, 29, 86, 98, 60, 149, 58, 169, 236, 53, 216, 185, 60, 152,
        ]);
        let _aes_iv = AesIv(3463705055);
        let sequence_number = 0;

        let expected_encrypted_packet = [
            1, 0, 51, 0, 0, 0, 0, 0, 107, 133, 50, 120, 59, 247, 93, 85, 108, 148, 57, 69, 152, 91,
            91, 123, 132, 64, 61, 60, 120, 238, 53, 108, 179, 210, 227, 52, 69, 244, 170, 34, 104,
            146, 169, 57, 37, 196, 207, 186, 20, 35, 130, 188, 96, 161, 68,
        ];

        // this is a sunshine hdr packet
        let expected_unencrypted_packet = [
            14, 1, 27, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0,
        ];

        // Encrypt test
        let mut encrypted_packet = vec![0; 1000];
        let encrypted_len = encrypt_server_control_packet_into(
            &crypto,
            ControlEncryptionMethod::Sunshine,
            aes_key,
            sequence_number,
            &expected_unencrypted_packet,
            &mut encrypted_packet,
        )
        .unwrap();
        assert_eq!(
            encrypted_packet[0..encrypted_len],
            expected_encrypted_packet
        );

        // Decrypt test
        let mut decrypted_packet = vec![0; 1000];
        let decrypted_len = decrypt_server_control_packet_into(
            &crypto,
            ControlEncryptionMethod::Sunshine,
            aes_key,
            &expected_encrypted_packet,
            &mut decrypted_packet,
        )
        .unwrap();
        assert_eq!(
            decrypted_packet[0..decrypted_len],
            expected_unencrypted_packet
        );
    }

    #[cfg(feature = "openssl")]
    #[test]
    fn clientbound_packet_sunshine_openssl() {
        use crate::crypto::openssl::OpenSSLCryptoBackend;

        test_clientbound_packet(OpenSSLCryptoBackend);
    }

    fn test_serverbound_packet<Crypto>(crypto: Crypto)
    where
        Crypto: CryptoBackend,
        Crypto::Error: Error + 'static,
    {
        let aes_key = AesKey([
            198, 75, 90, 29, 86, 98, 60, 149, 58, 169, 236, 53, 216, 185, 60, 152,
        ]);
        let _aes_iv = AesIv(3463705055);
        let sequence_number = 0;

        let expected_encrypted_packet = [
            1, 0, 28, 0, 0, 0, 0, 0, 110, 233, 241, 40, 195, 220, 114, 162, 56, 80, 132, 190, 247,
            171, 99, 146, 131, 157, 245, 189, 110, 44, 20, 54,
        ];

        // this is a sunshine periodic ping packet
        let expected_unencrypted_packet = [0, 2, 4, 0, 0, 0, 0, 0];

        // Encrypt test
        let mut encrypted_packet = vec![0; 1000];
        let encrypted_len = encrypt_client_control_packet_into(
            &crypto,
            ControlEncryptionMethod::Sunshine,
            aes_key,
            sequence_number,
            &expected_unencrypted_packet,
            &mut encrypted_packet,
        )
        .unwrap();
        assert_eq!(
            encrypted_packet[0..encrypted_len],
            expected_encrypted_packet
        );

        // Decrypt test
        let mut decrypted_packet = vec![0; 1000];
        let decrypted_len = decrypt_client_control_packet_into(
            &crypto,
            ControlEncryptionMethod::Sunshine,
            aes_key,
            &expected_encrypted_packet,
            &mut decrypted_packet,
        )
        .unwrap();
        assert_eq!(
            decrypted_packet[0..decrypted_len],
            expected_unencrypted_packet
        );
    }

    #[cfg(feature = "openssl")]
    #[test]
    fn serverbound_packet_sunshine_openssl() {
        use crate::crypto::openssl::OpenSSLCryptoBackend;

        test_serverbound_packet(OpenSSLCryptoBackend);
    }
}
