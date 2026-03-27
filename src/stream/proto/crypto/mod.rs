use std::{fmt, fmt::Debug, sync::Arc};

use crate::stream::{AesIv, AesKey};

#[cfg(test)]
#[allow(clippy::unwrap_used)]
pub(crate) mod test;

pub enum CipherAlgorithm {
    Aes128Cbc,
    Aes128Gcm,
}

pub trait CryptoBackend: Send + Sync {
    type Error;

    // TODO: make seperate encrypt_aes_cbc and encrypt_aes_gcm fns, same with decrypt

    /// Encrypt a message using the given crypto context and parameters:
    /// - For CBC, PKCS7 padding is applied automatically.
    /// - For GCM, an authentication tag is written to `tag`.
    fn encrypt(
        &self,
        algorithm: CipherAlgorithm,
        key: &[u8],
        iv: &[u8],
        tag: &mut [u8],
        input: &[u8],
        output: &mut [u8],
    ) -> Result<(), Self::Error>;

    /// Decrypt a message using the given crypto context and parameters:
    /// - For CBC, `output` must be large enough to hold PKCS7-padded output.
    /// - For GCM, the IV may change between calls unless its length changes,
    ///   in which case `CipherFlags::RESET_IV` must be set.
    fn decrypt(
        &self,
        algorithm: CipherAlgorithm,
        key: &[u8],
        iv: &[u8],
        tag: Option<&[u8]>, // Required for AEAD (e.g. GCM), unused for CBC
        input: &[u8],
        output: &mut [u8],
    ) -> Result<usize, Self::Error>;
}

// TODO: better name, use this inside the proto streams
pub struct EncryptionValues {
    // TODO: what is error?
    pub backend: Arc<dyn CryptoBackend<Error = ()>>,
    pub remote_aes_key: AesKey,
    pub remote_aes_iv: AesIv,
}

impl Debug for EncryptionValues {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "EncryptionValues {{ remote_aes_key: {:?}, remote_aes_iv: {:?} }}",
            self.remote_aes_key, self.remote_aes_iv
        )
    }
}

impl<T> CryptoBackend for Arc<T>
where
    T: CryptoBackend,
{
    type Error = T::Error;

    fn encrypt(
        &self,
        algorithm: CipherAlgorithm,
        key: &[u8],
        iv: &[u8],
        tag: &mut [u8],
        input: &[u8],
        output: &mut [u8],
    ) -> Result<(), Self::Error> {
        T::encrypt(&self, algorithm, key, iv, tag, input, output)
    }

    fn decrypt(
        &self,
        algorithm: CipherAlgorithm,
        key: &[u8],
        iv: &[u8],
        tag: Option<&[u8]>, // Required for AEAD (e.g. GCM), unused for CBC
        input: &[u8],
        output: &mut [u8],
    ) -> Result<usize, Self::Error> {
        T::decrypt(&self, algorithm, key, iv, tag, input, output)
    }
}
