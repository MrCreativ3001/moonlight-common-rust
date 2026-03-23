use std::fmt::Debug;

use crate::stream::proto::crypto::{CipherAlgorithm, CryptoBackend};

pub fn test_aes_cbc_roundtrip(backend: &impl CryptoBackend<Error = impl Debug>) {
    let key = [0x11u8; 16];
    let iv = [0x22u8; 16];

    let plaintext = b"hello world 123"; // forces padding

    let mut ciphertext = vec![0u8; 32];
    let mut decrypted = vec![0u8; 32];
    let mut tag = [0u8; 16]; // unused for CBC

    backend
        .encrypt(
            CipherAlgorithm::Aes128Cbc,
            &key,
            &iv,
            &mut tag,
            plaintext,
            &mut ciphertext,
        )
        .expect("encrypt failed");

    let len = backend
        .decrypt(
            CipherAlgorithm::Aes128Cbc,
            &key,
            &iv,
            None,
            &ciphertext,
            &mut decrypted,
        )
        .expect("decrypt failed");

    // strip PKCS7 manually
    let pad = decrypted[len - 1] as usize;
    let unpadded = &decrypted[..len - pad];

    assert_eq!(unpadded, plaintext);
}

pub fn test_aes_gcm_roundtrip(backend: &impl CryptoBackend<Error = impl Debug>) {
    let key = [0x33u8; 16];
    let iv = [0x44u8; 12];

    let plaintext = b"authenticated encryption test";

    let mut ciphertext = vec![0u8; plaintext.len()];
    let mut decrypted = vec![0u8; plaintext.len()];
    let mut tag = [0u8; 16];

    backend
        .encrypt(
            CipherAlgorithm::Aes128Gcm,
            &key,
            &iv,
            &mut tag,
            plaintext,
            &mut ciphertext,
        )
        .expect("encrypt failed");

    let len = backend
        .decrypt(
            CipherAlgorithm::Aes128Gcm,
            &key,
            &iv,
            Some(&tag),
            &ciphertext,
            &mut decrypted,
        )
        .expect("decrypt failed");

    assert_eq!(&decrypted[..len], plaintext);
}

pub fn test_gcm_tag_failure(backend: &impl CryptoBackend<Error = impl Debug>) {
    let key = [0x55u8; 16];
    let iv = [0x66u8; 12];

    let plaintext = b"tamper detection";

    let mut ciphertext = vec![0u8; plaintext.len()];
    let mut decrypted = vec![0u8; plaintext.len()];
    let mut tag = [0u8; 16];

    backend
        .encrypt(
            CipherAlgorithm::Aes128Gcm,
            &key,
            &iv,
            &mut tag,
            plaintext,
            &mut ciphertext,
        )
        .unwrap();

    // tamper with ciphertext
    ciphertext[0] ^= 0xFF;

    let result = backend.decrypt(
        CipherAlgorithm::Aes128Gcm,
        &key,
        &iv,
        Some(&tag),
        &ciphertext,
        &mut decrypted,
    );

    assert!(result.is_err(), "tampering should fail");
}
