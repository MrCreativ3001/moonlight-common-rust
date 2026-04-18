use crate::stream::proto::crypto::{CryptoBackend, round_to_pkcs7_safe_len};

pub fn test_aes_cbc_roundtrip(backend: &impl CryptoBackend) {
    let key = [0x11u8; 16];
    let iv = [0x22u8; 16];

    let expected_plaintext = b"hello world 123"; // forces padding
    let expected_ciphertext = [
        74, 33, 162, 224, 94, 5, 114, 62, 77, 96, 165, 123, 88, 229, 164, 179,
    ];

    let mut ciphertext = vec![0u8; round_to_pkcs7_safe_len(expected_plaintext.len())];

    let len = backend
        .encrypt_aes_cbc(&key, &iv, expected_plaintext, &mut ciphertext)
        .expect("encrypt failed");

    assert_eq!(&ciphertext[0..len], expected_ciphertext.as_slice());

    let mut plaintext = vec![0u8; round_to_pkcs7_safe_len(ciphertext.len())];

    let len = backend
        .decrypt_aes_cbc(&key, &iv, &expected_ciphertext, &mut plaintext)
        .expect("decrypt failed");

    assert_eq!(len, expected_plaintext.len());
    assert_eq!(&plaintext[0..len], expected_plaintext.as_slice());
}

pub fn test_aes_gcm_roundtrip(backend: &impl CryptoBackend) {
    let key = [0x33u8; 16];
    let iv = [0x44u8; 12];

    let expected_plaintext = b"authenticated encryption test";
    let expected_ciphertext = [
        227, 170, 120, 14, 223, 202, 210, 171, 34, 114, 86, 177, 125, 88, 37, 74, 181, 51, 95, 5,
        3, 125, 45, 133, 23, 236, 116, 68, 128,
    ];
    let expected_tag = [
        231, 251, 174, 73, 134, 41, 168, 74, 197, 168, 48, 106, 12, 41, 147, 43,
    ];

    let mut plaintext = vec![0u8; expected_ciphertext.len()];
    let mut ciphertext = vec![0u8; expected_plaintext.len()];
    let mut tag = [0u8; 16];

    backend
        .encrypt_aes_gcm(&key, &iv, expected_plaintext, &mut ciphertext, &mut tag)
        .expect("encrypt failed");
    assert_eq!(&ciphertext, expected_ciphertext.as_slice());
    assert_eq!(tag, expected_tag);

    backend
        .decrypt_aes_gcm(&key, &iv, &ciphertext, &tag, &mut plaintext)
        .expect("decrypt failed");
    assert_eq!(&plaintext, expected_plaintext.as_slice());
}
