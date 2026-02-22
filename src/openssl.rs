//!
//! This module contains openssl implementations for the CryptoProviders this library has.
//!

use openssl::{
    cipher::Cipher,
    cipher_ctx::CipherCtx,
    error::ErrorStack,
    md::Md,
    md_ctx::MdCtx,
    pkey::{PKey, Private},
    sha::{sha1, sha256},
    x509::X509,
};

use crate::http::{
    ClientSecret, ServerIdentifier,
    pair::{HashAlgorithm, PairCryptoProvider},
};

pub struct OpenSSLCryptoProvider;

impl PairCryptoProvider for OpenSSLCryptoProvider {
    type Error = ErrorStack;

    fn hash(&self, algorithm: HashAlgorithm, data: &[u8], output: &mut [u8]) {
        match algorithm {
            HashAlgorithm::Sha1 => {
                let digest = sha1(data);
                output.copy_from_slice(&digest);
            }
            HashAlgorithm::Sha256 => {
                let digest = sha256(data);
                output.copy_from_slice(&digest);
            }
        }
    }

    fn encrypt_aes(&self, key: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, Self::Error> {
        let mut cipher_ctx = CipherCtx::new()?;

        cipher_ctx.encrypt_init(Some(Cipher::aes_128_ecb()), Some(key), None)?;
        cipher_ctx.set_padding(false);

        let mut output = Vec::new();
        cipher_ctx.cipher_update_vec(plaintext, &mut output)?;
        Ok(output)
    }

    fn decrypt_aes(&self, key: &[u8], ciphertext: &[u8]) -> Result<Vec<u8>, Self::Error> {
        let mut cipher_ctx = CipherCtx::new()?;

        cipher_ctx.decrypt_init(Some(Cipher::aes_128_ecb()), Some(key), None)?;
        cipher_ctx.set_padding(false);

        let mut decrypted = Vec::new();
        cipher_ctx.cipher_update_vec(ciphertext, &mut decrypted)?;

        Ok(decrypted)
    }

    fn verify_signature(
        &self,
        server_secret: &[u8],
        server_signature: &[u8],
        server_identifier: &ServerIdentifier,
    ) -> Result<bool, Self::Error> {
        let server_certificate = X509::from_der(server_identifier.to_pem().contents())?;

        let public_key = server_certificate.public_key()?;

        let mut md_ctx = MdCtx::new()?;

        md_ctx.digest_verify_init(Some(Md::sha256()), &public_key)?;
        md_ctx.digest_verify_update(server_secret)?;
        md_ctx.digest_verify_final(server_signature)
    }

    fn sign_data(&self, private_key: &ClientSecret, data: &[u8]) -> Result<Vec<u8>, Self::Error> {
        let private_key = PKey::<Private>::private_key_from_der(private_key.to_pem().contents())?;

        let mut md_ctx = MdCtx::new()?;

        md_ctx.digest_sign_init(Some(Md::sha256()), &private_key)?;
        md_ctx.digest_sign_update(data)?;

        let mut out = Vec::new();
        md_ctx.digest_sign_final_to_vec(&mut out)?;

        Ok(out)
    }
}
