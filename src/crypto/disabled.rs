use thiserror::Error;

use crate::{
    http::{
        ClientIdentifier, ClientSecret,
        pair::{HashAlgorithm, PairingCryptoBackend},
    },
    stream::proto::crypto::{CipherAlgorithm, CryptoBackend},
};

#[derive(Debug, Error)]
#[error("the cryptography operations have been disabled")]
pub struct CryptoBackendDisabledError;

#[derive(Debug, Clone)]
pub struct DisabledCryptoBackend;

impl PairingCryptoBackend for DisabledCryptoBackend {
    type Error = CryptoBackendDisabledError;

    fn generate_client_identity(&self) -> Result<(ClientIdentifier, ClientSecret), Self::Error> {
        Err(CryptoBackendDisabledError)
    }

    fn hash(
        &self,
        _algorithm: HashAlgorithm,
        _data: &[u8],
        _output: &mut [u8],
    ) -> Result<(), Self::Error> {
        Err(CryptoBackendDisabledError)
    }

    fn random_bytes(&self, _data: &mut [u8]) -> Result<(), Self::Error> {
        Err(CryptoBackendDisabledError)
    }

    fn encrypt_aes(&self, _key: &[u8], _plaintext: &[u8]) -> Result<Vec<u8>, Self::Error> {
        Err(CryptoBackendDisabledError)
    }

    fn decrypt_aes(&self, _key: &[u8], _ciphertext: &[u8]) -> Result<Vec<u8>, Self::Error> {
        Err(CryptoBackendDisabledError)
    }

    fn client_signature(
        &self,
        _client_certificate: &crate::http::ClientIdentifier,
    ) -> Result<Vec<u8>, Self::Error> {
        Err(CryptoBackendDisabledError)
    }

    fn server_signature(
        &self,
        _server_certificate: &crate::http::ServerIdentifier,
    ) -> Result<Vec<u8>, Self::Error> {
        Err(CryptoBackendDisabledError)
    }

    fn verify_signature(
        &self,
        _server_secret: &[u8],
        _server_signature: &[u8],
        _server_certificate: &crate::http::ServerIdentifier,
    ) -> Result<bool, Self::Error> {
        Err(CryptoBackendDisabledError)
    }

    fn sign_data(
        &self,
        _private_key: &crate::http::ClientSecret,
        _data: &[u8],
    ) -> Result<Vec<u8>, Self::Error> {
        Err(CryptoBackendDisabledError)
    }
}

impl CryptoBackend for DisabledCryptoBackend {
    type Error = CryptoBackendDisabledError;

    fn encrypt(
        &self,
        _algorithm: CipherAlgorithm,
        _key: &[u8],
        _iv: &[u8],
        _tag: &mut [u8],
        _input: &[u8],
        _output: &mut [u8],
    ) -> Result<(), Self::Error> {
        Err(CryptoBackendDisabledError)
    }
    fn decrypt(
        &self,
        _algorithm: CipherAlgorithm,
        _key: &[u8],
        _iv: &[u8],
        _tag: Option<&[u8]>, // Required for AEAD (e.g. GCM), unused for CBC
        _input: &[u8],
        _output: &mut [u8],
    ) -> Result<usize, Self::Error> {
        Err(CryptoBackendDisabledError)
    }
}
