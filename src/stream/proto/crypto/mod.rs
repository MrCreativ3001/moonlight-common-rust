use std::{error::Error, sync::Arc};

use thiserror::Error;

#[cfg(test)]
#[allow(clippy::unwrap_used)]
pub(crate) mod test;

pub enum CipherAlgorithm {
    Aes128Cbc,
    Aes128Gcm,
}

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
    ) -> Result<(), CryptoError>;

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
    ) -> Result<usize, CryptoError>;
}

impl<T> CryptoBackend for Arc<T>
where
    T: CryptoBackend + ?Sized,
{
    fn encrypt(
        &self,
        algorithm: CipherAlgorithm,
        key: &[u8],
        iv: &[u8],
        tag: &mut [u8],
        input: &[u8],
        output: &mut [u8],
    ) -> Result<(), CryptoError> {
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
    ) -> Result<usize, CryptoError> {
        T::decrypt(&self, algorithm, key, iv, tag, input, output)
    }
}
