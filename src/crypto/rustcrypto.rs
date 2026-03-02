//!
//! This module contains pure rust implementations for the CryptoBackends this library has.
//! It uses multiple different crates to achieve this.
//!

use std::{str::FromStr, time::Duration};

use aes::{
    Aes128,
    cipher::{BlockDecrypt, BlockEncrypt, KeyInit, generic_array::GenericArray},
};
use der::Decode;
use pem::Pem;
use pkcs8::{DecodePrivateKey, EncodePrivateKey, SubjectPublicKeyInfo, der::Encode};
use rand::RngCore;
use rsa::{
    RsaPrivateKey, RsaPublicKey,
    pkcs1v15::{Signature, SigningKey, VerifyingKey},
    rand_core::OsRng,
};
use sha1::{Digest, Sha1};
use sha2::Sha256;
use signature::{SignatureEncoding, Signer, Verifier};
use spki::DecodePublicKey;
use thiserror::Error;
use tracing::{Level, Span, debug_span, instrument, trace};
use x509_cert::{
    Certificate,
    builder::{Builder, CertificateBuilder, Profile},
    name::Name,
    serial_number::SerialNumber,
    time::Validity,
};

use crate::http::{
    ClientIdentifier, ClientSecret, ServerIdentifier,
    pair::{HashAlgorithm, PairingCryptoBackend},
};

#[derive(Debug, Error)]
pub enum RustCryptoError {
    #[error("rand: {0}")]
    Rand(#[from] rand::Error),
    #[error("certificate builder: {0}")]
    CertificateBuilder(#[from] x509_cert::builder::Error),
    #[error("rsa: {0}")]
    Rsa(#[from] rsa::Error),
    #[error("der: {0}")]
    Der(#[from] der::Error),
    #[error("spki: {0}")]
    Spki(#[from] spki::Error),
    #[error("signature: {0}")]
    Signature(#[from] signature::Error),
    #[error("invalid bitstring signature")]
    InvalidBitStringSignature,
    #[error("pkcs8: {0}")]
    Pkcs8(#[from] pkcs8::Error),
}

#[derive(Debug)]
pub struct RustCryptoBackend {
    span: Span,
}

impl Default for RustCryptoBackend {
    fn default() -> Self {
        Self {
            span: debug_span!("moonlight::crypto::rustcrypto"),
        }
    }
}

fn secure_rng() -> Result<OsRng, RustCryptoError> {
    Ok(OsRng)
}

impl PairingCryptoBackend for RustCryptoBackend {
    type Error = RustCryptoError;

    #[cfg_attr(not(feature = "__tracing_sensitive"), instrument(level = Level::TRACE, parent = &self.span, skip_all, err))]
    #[cfg_attr(feature = "__tracing_sensitive", instrument(level = Level::TRACE, parent = &self.span, skip(self, output), ret, err))]
    fn hash(
        &self,
        algorithm: HashAlgorithm,
        data: &[u8],
        output: &mut [u8],
    ) -> Result<(), Self::Error> {
        match algorithm {
            HashAlgorithm::Sha1 => {
                let digest = Sha1::digest(data);
                output.copy_from_slice(&digest);
            }
            HashAlgorithm::Sha256 => {
                let digest = Sha256::digest(data);
                output.copy_from_slice(&digest);
            }
        }

        trace!(output = ?output);

        Ok(())
    }

    #[cfg_attr(not(feature = "__tracing_sensitive"), instrument(level = Level::TRACE, parent = &self.span, skip_all, err))]
    #[cfg_attr(feature = "__tracing_sensitive", instrument(level = Level::TRACE, parent = &self.span, skip(self, data), ret, err))]
    fn random_bytes(&self, data: &mut [u8]) -> Result<(), Self::Error> {
        secure_rng()?.try_fill_bytes(data)?;

        trace!(data = ?data);

        Ok(())
    }

    #[cfg_attr(not(feature = "__tracing_sensitive"), instrument(level = Level::TRACE, parent = &self.span, skip_all, err))]
    #[cfg_attr(feature = "__tracing_sensitive", instrument(level = Level::TRACE, parent = &self.span, skip(self), ret, err))]
    fn generate_client_identity(&self) -> Result<(ClientIdentifier, ClientSecret), Self::Error> {
        // Generate Private Key
        let private_key = RsaPrivateKey::new(&mut secure_rng()?, 2048)?;
        let private_key_der = private_key.to_pkcs8_der()?;
        let private_key_pem = Pem::new("PRIVATE KEY", private_key_der.as_bytes());

        // Generate Certificate
        let public_key = private_key.to_public_key();

        let serial_number = SerialNumber::from(1u32);
        let validity = Validity::from_now(Duration::from_secs(60 * 60 * 24 * 365))?;
        // This is a valid subject
        #[allow(clippy::unwrap_used)]
        let subject =
            Name::from_str("C=US,ST=CA,L=San Francisco,O=Example Corp,CN=example.com").unwrap();
        let subject_public_key_info = SubjectPublicKeyInfo::from_key(public_key)?;

        let cert_signer = SigningKey::<Sha256>::new(private_key);

        let certificate = CertificateBuilder::new(
            Profile::Root,
            serial_number,
            validity,
            subject,
            subject_public_key_info,
            &cert_signer,
        )?
        .build()?;

        let certificate_der = certificate.to_der()?;
        let certificate_pem = Pem::new("CERTIFICATE", certificate_der);

        Ok((
            // We're trusting rustcrypto to give us correct values
            #[allow(clippy::unwrap_used)]
            ClientIdentifier::from_pem(certificate_pem),
            ClientSecret::from_pem(private_key_pem),
        ))
    }

    #[cfg_attr(not(feature = "__tracing_sensitive"), instrument(level = Level::TRACE, parent = &self.span, skip_all, err))]
    #[cfg_attr(feature = "__tracing_sensitive", instrument(level = Level::TRACE, parent = &self.span, skip(self), ret, err))]
    fn encrypt_aes(&self, key: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, Self::Error> {
        // TODO: maybe return this as an error
        assert_eq!(key.len(), 16);
        assert_eq!(plaintext.len() % 16, 0);

        let cipher = Aes128::new(GenericArray::from_slice(key));

        let mut out = plaintext.to_vec();

        for chunk in out.chunks_mut(16) {
            let block = GenericArray::from_mut_slice(chunk);
            cipher.encrypt_block(block);
        }

        Ok(out)
    }

    #[cfg_attr(not(feature = "__tracing_sensitive"), instrument(level = Level::TRACE, parent = &self.span, skip_all, err))]
    #[cfg_attr(feature = "__tracing_sensitive", instrument(level = Level::TRACE, parent = &self.span, skip(self), ret, err))]
    fn decrypt_aes(&self, key: &[u8], ciphertext: &[u8]) -> Result<Vec<u8>, Self::Error> {
        // TODO: maybe return this as an error
        assert_eq!(key.len(), 16);
        assert_eq!(ciphertext.len() % 16, 0);

        let cipher = Aes128::new(GenericArray::from_slice(key));

        let mut out = ciphertext.to_vec();

        for chunk in out.chunks_mut(16) {
            let block = GenericArray::from_mut_slice(chunk);
            cipher.decrypt_block(block);
        }

        Ok(out)
    }

    #[cfg_attr(not(feature = "__tracing_sensitive"), instrument(level = Level::TRACE, parent = &self.span, skip_all, err))]
    #[cfg_attr(feature = "__tracing_sensitive", instrument(level = Level::TRACE, parent = &self.span, skip(self), ret, err))]
    fn client_signature(
        &self,
        client_certificate: &ClientIdentifier,
    ) -> Result<Vec<u8>, Self::Error> {
        let client_certificate = Certificate::from_der(client_certificate.to_pem().contents())?;

        Ok(client_certificate
            .signature
            .as_bytes()
            .ok_or(RustCryptoError::InvalidBitStringSignature)?
            .to_vec())
    }

    #[cfg_attr(not(feature = "__tracing_sensitive"), instrument(level = Level::TRACE, parent = &self.span, skip_all, err))]
    #[cfg_attr(feature = "__tracing_sensitive", instrument(level = Level::TRACE, parent = &self.span, skip(self), ret, err))]
    fn server_signature(
        &self,
        server_certificate: &ServerIdentifier,
    ) -> Result<Vec<u8>, Self::Error> {
        let server_certificate = Certificate::from_der(server_certificate.to_pem().contents())?;

        Ok(server_certificate
            .signature
            .as_bytes()
            .ok_or(RustCryptoError::InvalidBitStringSignature)?
            .to_vec())
    }

    #[cfg_attr(not(feature = "__tracing_sensitive"), instrument(level = Level::TRACE, parent = &self.span, skip_all, err))]
    #[cfg_attr(feature = "__tracing_sensitive", instrument(level = Level::TRACE, parent = &self.span, skip(self), ret, err))]
    fn verify_signature(
        &self,
        server_secret: &[u8],
        server_signature: &[u8],
        server_identifier: &ServerIdentifier,
    ) -> Result<bool, Self::Error> {
        let certificate = Certificate::from_der(server_identifier.to_pem().contents())?;

        // Extract SubjectPublicKeyInfo
        let spki = certificate.tbs_certificate.subject_public_key_info;

        // Convert SPKI → RSA public key
        let public_key = RsaPublicKey::from_public_key_der(&spki.to_der()?)?;

        let verifying_key = VerifyingKey::<Sha256>::new(public_key);

        Ok(verifying_key
            .verify(server_secret, &Signature::try_from(server_signature)?)
            .is_ok())
    }

    #[cfg_attr(not(feature = "__tracing_sensitive"), instrument(level = Level::TRACE, parent = &self.span, skip_all, err))]
    #[cfg_attr(feature = "__tracing_sensitive", instrument(level = Level::TRACE, parent = &self.span, skip(self), ret, err))]
    fn sign_data(&self, private_key: &ClientSecret, data: &[u8]) -> Result<Vec<u8>, Self::Error> {
        let private_key = RsaPrivateKey::from_pkcs8_der(private_key.to_pem().contents())?;

        let signing_key = SigningKey::<Sha256>::new(private_key);

        let signature = signing_key.sign(data);

        Ok(signature.to_vec())
    }
}
