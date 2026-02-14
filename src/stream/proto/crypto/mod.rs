use std::sync::Arc;

use bitflags::bitflags;
use thiserror::Error;

use crate::stream::{AesIv, AesKey};

#[derive(Debug, Error)]
pub enum CryptoError {
    #[error("invalid key length")]
    InvalidKeyLength,
    #[error("invalid iv length")]
    InvalidIvLength,
    #[error("invalid tag length")]
    InvalidTagLength,
    #[error("authentication failed")]
    AuthenticationFailed,
    #[error("buffer too small")]
    BufferTooSmall,
    #[error("unsupported operation")]
    UnsupportedOperation,
    #[error("backend error")]
    BackendError,
}

pub enum CipherAlgorithm {
    AesCbc,
    AesGcm,
}

bitflags! {
    /// Cipher behavior flags
    pub struct CipherFlags: u32 {
        /// Reset IV
        const RESET_IV = 0b0001;
    }
}

pub trait CryptoContext: Send + Sync {
    /// Encrypt a message using the given crypto context and parameters:
    /// - For CBC, PKCS7 padding is applied automatically.
    /// - For GCM, an authentication tag is written to `tag`.
    fn encrypt(
        &self,
        algorithm: CipherAlgorithm,
        flags: (),
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
        flags: CipherFlags,
        key: &[u8],
        iv: &[u8],
        tag: Option<&[u8]>, // Required for AEAD (e.g. GCM), unused for CBC
        input: &[u8],
        output: &mut [u8],
    ) -> Result<usize, CryptoError>;
}

// TODO: better name, use this inside the proto streams
pub struct EncryptionValues {
    pub context: Arc<dyn CryptoContext>,
    pub remote_aes_key: AesKey,
    pub remote_aes_iv: AesIv,
}
