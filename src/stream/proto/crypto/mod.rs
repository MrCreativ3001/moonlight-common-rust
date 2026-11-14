use std::{fmt::Debug, sync::Arc};

use crate::error::Error;

#[cfg(test)]
pub(crate) mod test;

// TODO: switch to in place encryption

pub trait CryptoBackend: Debug + Send + Sync {
    /// Encrypt using AES-GCM.
    /// Writes ciphertext to `output` and authentication tag to `tag`.
    fn encrypt_aes_gcm(
        &self,
        key: &[u8],
        iv: &[u8],
        input: &[u8],
        output: &mut [u8],
        tag: &mut [u8],
    ) -> Result<(), Error>;

    /// Decrypt using AES-GCM.
    /// Verifies `tag` before returning plaintext length.
    fn decrypt_aes_gcm(
        &self,
        key: &[u8],
        iv: &[u8],
        input: &[u8],
        tag: &[u8],
        output: &mut [u8],
    ) -> Result<(), Error>;

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
    ) -> Result<usize, Error>;

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
    ) -> Result<usize, Error>;
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
    ) -> Result<(), Error> {
        T::encrypt_aes_gcm(self, key, iv, input, output, tag)
    }

    fn decrypt_aes_gcm(
        &self,
        key: &[u8],
        iv: &[u8],
        input: &[u8],
        tag: &[u8],
        output: &mut [u8],
    ) -> Result<(), Error> {
        T::decrypt_aes_gcm(self, key, iv, input, tag, output)
    }

    fn encrypt_aes_cbc(
        &self,
        key: &[u8],
        iv: &[u8],
        input: &[u8],
        output: &mut [u8],
    ) -> Result<usize, Error> {
        T::encrypt_aes_cbc(self, key, iv, input, output)
    }

    fn decrypt_aes_cbc(
        &self,
        key: &[u8],
        iv: &[u8],
        input: &[u8],
        output: &mut [u8],
    ) -> Result<usize, Error> {
        T::decrypt_aes_cbc(self, key, iv, input, output)
    }
}

const BLOCK_SIZE: usize = 16;

/// References:
/// - <https://github.com/moonlight-stream/moonlight-common-c/blob/62687809b1f7410c3db4be2527503a54ae408d70/src/PlatformCrypto.h#L22>
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
