use std::{error::Error, sync::Arc};

use thiserror::Error;

#[cfg(test)]
#[allow(clippy::unwrap_used)]
pub(crate) mod test;

#[derive(Debug, Error)]
#[error("{inner}")]
pub struct CryptoError {
    inner: Box<dyn Error + Send + 'static>,
}

impl CryptoError {
    pub fn from_error<T>(value: T) -> Self
    where
        T: Error + Send + 'static,
    {
        Self {
            inner: Box::new(value),
        }
    }
}

pub trait CryptoBackend: Send + Sync {
    /// Encrypt using AES-GCM.
    /// Writes ciphertext to `output` and authentication tag to `tag`.
    fn encrypt_aes_gcm(
        &self,
        key: &[u8],
        iv: &[u8],
        input: &[u8],
        output: &mut [u8],
        tag: &mut [u8],
    ) -> Result<(), CryptoError>;

    /// Decrypt using AES-GCM.
    /// Verifies `tag` before returning plaintext length.
    fn decrypt_aes_gcm(
        &self,
        key: &[u8],
        iv: &[u8],
        input: &[u8],
        tag: &[u8],
        output: &mut [u8],
    ) -> Result<(), CryptoError>;

    /// Encrypt using AES-CBC with PKCS7 padding.
    /// Returns number of bytes written after adding padding.
    ///
    /// The output buffer must at least have the size returned from [round_to_pkcs7_safe_len](crate::crypto::round_to_pkcs7_safe_len).
    fn encrypt_aes_cbc(
        &self,
        key: &[u8],
        iv: &[u8],
        input: &[u8],
        output: &mut [u8],
    ) -> Result<usize, CryptoError>;

    /// Decrypt using AES-CBC with PKCS7 padding.
    /// Returns number of bytes written after unpadding.
    ///
    /// The output buffer must at least have the size returned from [round_to_pkcs7_safe_len](crate::crypto::round_to_pkcs7_safe_len).
    fn decrypt_aes_cbc(
        &self,
        key: &[u8],
        iv: &[u8],
        input: &[u8],
        output: &mut [u8],
    ) -> Result<usize, CryptoError>;
}

impl<T> CryptoBackend for Arc<T>
where
    T: CryptoBackend + ?Sized,
{
    fn encrypt_aes_gcm(
        &self,
        key: &[u8],
        iv: &[u8],
        input: &[u8],
        output: &mut [u8],
        tag: &mut [u8],
    ) -> Result<(), CryptoError> {
        T::encrypt_aes_gcm(self, key, iv, input, output, tag)
    }

    fn decrypt_aes_gcm(
        &self,
        key: &[u8],
        iv: &[u8],
        input: &[u8],
        tag: &[u8],
        output: &mut [u8],
    ) -> Result<(), CryptoError> {
        T::decrypt_aes_gcm(self, key, iv, input, tag, output)
    }

    fn encrypt_aes_cbc(
        &self,
        key: &[u8],
        iv: &[u8],
        input: &[u8],
        output: &mut [u8],
    ) -> Result<usize, CryptoError> {
        T::encrypt_aes_cbc(self, key, iv, input, output)
    }

    fn decrypt_aes_cbc(
        &self,
        key: &[u8],
        iv: &[u8],
        input: &[u8],
        output: &mut [u8],
    ) -> Result<usize, CryptoError> {
        T::decrypt_aes_cbc(self, key, iv, input, output)
    }
}

const BLOCK_SIZE: usize = 16;

/// References:
/// - https://github.com/moonlight-stream/moonlight-common-c/blob/62687809b1f7410c3db4be2527503a54ae408d70/src/PlatformCrypto.h#L22
pub const fn round_to_pkcs7_padded_len(x: usize) -> usize {
    x.div_ceil(BLOCK_SIZE) * BLOCK_SIZE
}

/// This function should be used to know the amount that MUST be at least allocated for aes cbc.
///
/// See also:
/// - [encrypt_aes_cbc](crate::stream::proto::crypto::CryptoBackend::encrypt_aes_cbc)
/// - [decrypt_aes_cbc](crate::stream::proto::crypto::CryptoBackend::decrypt_aes_cbc)
pub const fn round_to_pkcs7_safe_len(x: usize) -> usize {
    round_to_pkcs7_padded_len(x) + BLOCK_SIZE
}

/// Aes Cbc uses custom padding.
///
/// The plaintext buffer must at least have the size returned from [round_to_pkcs7_safe_len](crate::crypto::round_to_pkcs7_safe_len).
///
/// References:
/// - https://github.com/moonlight-stream/moonlight-common-c/blob/7b026e77be62175104640e7e722b758df6d3d0d7/src/PlatformCrypto.c#L21-L28
pub fn add_pkcs_7_padding(plaintext: &mut [u8], plaintext_len: usize) -> usize {
    let padded_len = round_to_pkcs7_padded_len(plaintext_len);

    // The padding byte contains the total number of padding bytes added.
    let padding = padded_len - plaintext_len;

    plaintext[plaintext_len..padded_len].fill(padding as u8);

    padded_len
}

/// Aes Cbc uses custom padding.
/// This will return the size of the plaintext without padding.
///
/// References:
/// - https://github.com/moonlight-stream/moonlight-common-c/blob/7b026e77be62175104640e7e722b758df6d3d0d7/src/PlatformCrypto.c#L21-L28
pub fn remove_pkcs_7_padding(plaintext_full: &mut [u8]) -> usize {
    debug_assert!(
        !plaintext_full.is_empty(),
        "cannot remove padding from empty plaintext"
    );

    let pad_len = plaintext_full.last().copied().unwrap_or(0);

    let plaintext_len = plaintext_full.len().saturating_sub(pad_len as usize);

    // debug assert correct padding
    for pad_byte in &plaintext_full[plaintext_len..] {
        debug_assert_eq!(
            pad_len, *pad_byte,
            "all bytes after the plaintext must be the padding length"
        );
    }

    plaintext_len
}
