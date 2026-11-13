//!
//! This module contains pure rust implementations for the CryptoBackends this library has.
//! It uses multiple different crates to achieve this.
//!

use std::{str::FromStr, time::Duration};

use aes::{
    Aes128,
    cipher::{
        BlockDecrypt, BlockEncrypt, InvalidLength, KeyInit, block_padding::UnpadError,
        generic_array::GenericArray, inout::PadError,
    },
};
use der::Decode;
use pem::Pem;
use pkcs8::{DecodePrivateKey, EncodePrivateKey, SubjectPublicKeyInfo, der::Encode};
use rand_v8::RngCore;
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
use tracing::{Level, instrument, trace};
use x509_cert::{
    Certificate,
    builder::{Builder, CertificateBuilder, Profile},
    name::Name,
    serial_number::SerialNumber,
    time::Validity,
};

use crate::{
    error::Error,
    http::{
        ClientIdentifier, ClientSecret, ServerIdentifier,
        pair::{HashAlgorithm, PairingCryptoBackend},
    },
};

#[derive(Debug, Error)]
pub enum RustCryptoError {
    #[error("rand: {0}")]
    Rand(#[from] rand_v8::Error),
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
    #[error("aes gcm: {0}")]
    AesGcm(#[from] aes_gcm::aead::Error),
    #[error("aes cbc: invalid length")]
    AesCbcInvalidLength(#[from] InvalidLength),
    #[error("aes cbc: pad error")]
    AesCbcPadError(#[from] PadError),
    #[error("aes cbc: unpad error")]
    AesCbcUnpadError(#[from] UnpadError),
}

fn err(value: impl Into<RustCryptoError>) -> Error {
    value.into().into()
}

impl From<RustCryptoError> for Error {
    fn from(value: RustCryptoError) -> Self {
        Self::Other(value.into())
    }
}

#[derive(Debug, Clone)]
pub struct RustCryptoBackend;

fn secure_rng() -> Result<OsRng, RustCryptoError> {
    Ok(OsRng)
}

impl PairingCryptoBackend for RustCryptoBackend {
    #[cfg_attr(not(feature = "__tracing_sensitive"), instrument(level = Level::TRACE, skip_all, err))]
    #[cfg_attr(feature = "__tracing_sensitive", instrument(level = Level::TRACE, skip(self, output), ret, err))]
    fn hash(&self, algorithm: HashAlgorithm, data: &[u8], output: &mut [u8]) -> Result<(), Error> {
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

    #[cfg_attr(not(feature = "__tracing_sensitive"), instrument(level = Level::TRACE, skip_all, err))]
    #[cfg_attr(feature = "__tracing_sensitive", instrument(level = Level::TRACE, skip(self, data), ret, err))]
    fn random_bytes(&self, data: &mut [u8]) -> Result<(), Error> {
        secure_rng()?.try_fill_bytes(data).map_err(err)?;

        trace!(data = ?data);

        Ok(())
    }

    #[cfg_attr(not(feature = "__tracing_sensitive"), instrument(level = Level::TRACE, skip_all, err))]
    #[cfg_attr(feature = "__tracing_sensitive", instrument(level = Level::TRACE, skip(self), ret, err))]
    fn generate_client_identity(&self) -> Result<(ClientIdentifier, ClientSecret), Error> {
        // Generate Private Key
        let private_key = RsaPrivateKey::new(&mut secure_rng()?, 2048).map_err(err)?;
        let private_key_der = private_key.to_pkcs8_der().map_err(err)?;
        let private_key_pem = Pem::new("PRIVATE KEY", private_key_der.as_bytes());

        // Generate Certificate
        let public_key = private_key.to_public_key();

        let serial_number = SerialNumber::from(1u32);
        let validity = Validity::from_now(Duration::from_secs(60 * 60 * 24 * 365)).map_err(err)?;
        // This is a valid subject
        #[allow(clippy::unwrap_used)]
        let subject =
            Name::from_str("C=US,ST=CA,L=San Francisco,O=Example Corp,CN=example.com").unwrap();
        let subject_public_key_info = SubjectPublicKeyInfo::from_key(public_key).map_err(err)?;

        let cert_signer = SigningKey::<Sha256>::new(private_key);

        let certificate = CertificateBuilder::new(
            Profile::Root,
            serial_number,
            validity,
            subject,
            subject_public_key_info,
            &cert_signer,
        )
        .map_err(err)?
        .build()
        .map_err(err)?;

        let certificate_der = certificate.to_der().map_err(err)?;
        let certificate_pem = Pem::new("CERTIFICATE", certificate_der);

        Ok((
            // We're trusting rustcrypto to give us correct values
            #[allow(clippy::unwrap_used)]
            ClientIdentifier::from_pem(certificate_pem),
            ClientSecret::from_pem(private_key_pem),
        ))
    }

    #[cfg_attr(not(feature = "__tracing_sensitive"), instrument(level = Level::TRACE, skip_all, err))]
    #[cfg_attr(feature = "__tracing_sensitive", instrument(level = Level::TRACE, skip(self), ret, err))]
    fn encrypt_aes(&self, key: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, Error> {
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

    #[cfg_attr(not(feature = "__tracing_sensitive"), instrument(level = Level::TRACE, skip_all, err))]
    #[cfg_attr(feature = "__tracing_sensitive", instrument(level = Level::TRACE, skip(self), ret, err))]
    fn decrypt_aes(&self, key: &[u8], ciphertext: &[u8]) -> Result<Vec<u8>, Error> {
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

    #[cfg_attr(not(feature = "__tracing_sensitive"), instrument(level = Level::TRACE, skip_all, err))]
    #[cfg_attr(feature = "__tracing_sensitive", instrument(level = Level::TRACE, skip(self), ret, err))]
    fn client_signature(&self, client_certificate: &ClientIdentifier) -> Result<Vec<u8>, Error> {
        let client_certificate =
            Certificate::from_der(client_certificate.to_pem().contents()).map_err(err)?;

        Ok(client_certificate
            .signature
            .as_bytes()
            .ok_or(RustCryptoError::InvalidBitStringSignature)?
            .to_vec())
    }

    #[cfg_attr(not(feature = "__tracing_sensitive"), instrument(level = Level::TRACE, skip_all, err))]
    #[cfg_attr(feature = "__tracing_sensitive", instrument(level = Level::TRACE, skip(self), ret, err))]
    fn server_signature(&self, server_certificate: &ServerIdentifier) -> Result<Vec<u8>, Error> {
        let server_certificate =
            Certificate::from_der(server_certificate.to_pem().contents()).map_err(err)?;

        Ok(server_certificate
            .signature
            .as_bytes()
            .ok_or(RustCryptoError::InvalidBitStringSignature)?
            .to_vec())
    }

    #[cfg_attr(not(feature = "__tracing_sensitive"), instrument(level = Level::TRACE, skip_all, err))]
    #[cfg_attr(feature = "__tracing_sensitive", instrument(level = Level::TRACE, skip(self), ret, err))]
    fn verify_signature(
        &self,
        server_secret: &[u8],
        server_signature: &[u8],
        server_identifier: &ServerIdentifier,
    ) -> Result<bool, Error> {
        let certificate =
            Certificate::from_der(server_identifier.to_pem().contents()).map_err(err)?;

        let spki = certificate.tbs_certificate.subject_public_key_info;

        let public_key =
            RsaPublicKey::from_public_key_der(&spki.to_der().map_err(err)?).map_err(err)?;

        let verifying_key = VerifyingKey::<Sha256>::new(public_key);

        Ok(verifying_key
            .verify(
                server_secret,
                &Signature::try_from(server_signature).map_err(err)?,
            )
            .is_ok())
    }

    #[cfg_attr(not(feature = "__tracing_sensitive"), instrument(level = Level::TRACE, skip_all, err))]
    #[cfg_attr(feature = "__tracing_sensitive", instrument(level = Level::TRACE, skip(self), ret, err))]
    fn sign_data(&self, private_key: &ClientSecret, data: &[u8]) -> Result<Vec<u8>, Error> {
        let private_key =
            RsaPrivateKey::from_pkcs8_der(private_key.to_pem().contents()).map_err(err)?;

        let signing_key = SigningKey::<Sha256>::new(private_key);

        let signature = signing_key.sign(data);

        Ok(signature.to_vec())
    }
}

#[cfg(feature = "stream-proto")]
mod proto {
    use super::err;
    use aes::{
        Aes128,
        cipher::{BlockDecryptMut, BlockEncryptMut, KeyIvInit, block_padding::Pkcs7, consts::U16},
    };
    use aes_gcm::{AeadInPlace, Aes128Gcm, AesGcm, KeyInit};
    use cbc::{Decryptor, Encryptor};

    use crate::{
        crypto::rustcrypto::RustCryptoBackend, error::Error, stream::proto::crypto::CryptoBackend,
    };

    impl CryptoBackend for RustCryptoBackend {
        fn encrypt_aes_gcm(
            &self,
            key: &[u8],
            iv: &[u8],
            input: &[u8],
            output: &mut [u8],
            tag: &mut [u8],
        ) -> Result<(), Error> {
            debug_assert_eq!(key.len(), 16, "AES-128 key must be 16 bytes");
            debug_assert!(
                matches!(iv.len(), 12 | 16),
                "GCM nonce must be 12 or 16 bytes"
            );
            debug_assert_eq!(tag.len(), 16, "GCM tag must be 16 bytes");
            debug_assert!(
                output.len() >= input.len(),
                "output buffer too small: output({}), input({})",
                output.len(),
                input.len()
            );

            let new_tag = if iv.len() == 12 {
                let cipher = Aes128Gcm::new(key.into());

                output[0..input.len()].copy_from_slice(input);

                cipher
                    .encrypt_in_place_detached(iv.into(), &[], &mut output[0..input.len()])
                    .map_err(err)?
            } else if iv.len() == 16 {
                let cipher = AesGcm::<Aes128, U16>::new(key.into());

                output[0..input.len()].copy_from_slice(input);

                cipher
                    .encrypt_in_place_detached(iv.into(), &[], &mut output[0..input.len()])
                    .map_err(err)?
            } else {
                unreachable!()
            };

            tag.copy_from_slice(&new_tag);

            Ok(())
        }

        fn decrypt_aes_gcm(
            &self,
            key: &[u8],
            iv: &[u8],
            input: &[u8],
            tag: &[u8],
            output: &mut [u8],
        ) -> Result<(), Error> {
            debug_assert_eq!(key.len(), 16, "AES-128 key must be 16 bytes");
            debug_assert!(
                matches!(iv.len(), 12 | 16),
                "GCM nonce must be 12 or 16 bytes"
            );
            debug_assert_eq!(tag.len(), 16, "GCM tag must be 16 bytes");
            debug_assert!(output.len() >= input.len(), "output buffer too small");

            if iv.len() == 12 {
                let cipher = Aes128Gcm::new(key.into());

                output[0..input.len()].copy_from_slice(input);

                cipher
                    .decrypt_in_place_detached(
                        iv.into(),
                        &[],
                        &mut output[0..input.len()],
                        tag.into(),
                    )
                    .map_err(|_| Error::DecryptFailed("aes gcm"))?;
            } else if iv.len() == 16 {
                let cipher = AesGcm::<Aes128, U16>::new(key.into());

                output[0..input.len()].copy_from_slice(input);

                cipher
                    .decrypt_in_place_detached(
                        iv.into(),
                        &[],
                        &mut output[0..input.len()],
                        tag.into(),
                    )
                    .map_err(|_| Error::DecryptFailed("aes gcm"))?;
            }

            Ok(())
        }

        fn encrypt_aes_cbc(
            &self,
            key: &[u8],
            iv: &[u8],
            input: &[u8],
            output: &mut [u8],
        ) -> Result<usize, Error> {
            let cipher = Encryptor::<Aes128>::new_from_slices(key, iv).map_err(err)?;

            let len = cipher
                .encrypt_padded_b2b_mut::<Pkcs7>(input, output)
                .map_err(err)?
                .len();

            Ok(len)
        }

        fn decrypt_aes_cbc(
            &self,
            key: &[u8],
            iv: &[u8],
            input: &[u8],
            output: &mut [u8],
        ) -> Result<usize, Error> {
            let cipher = Decryptor::<Aes128>::new_from_slices(key, iv).map_err(err)?;

            let len = cipher
                .decrypt_padded_b2b_mut::<Pkcs7>(input, output)
                .map_err(err)?
                .len();

            Ok(len)
        }
    }

    #[cfg(test)]
    mod test {
        use crate::stream::proto::crypto::test::{
            test_aes_cbc_roundtrip, test_aes_gcm_roundtrip_nonce_12,
            test_aes_gcm_roundtrip_nonce_16,
        };

        use super::*;

        #[test]
        fn rustcrypto_cbc() {
            let backend = RustCryptoBackend;
            test_aes_cbc_roundtrip(&backend);
        }

        #[test]
        fn rustcrypto_gcm_nonce_12() {
            let backend = RustCryptoBackend;
            test_aes_gcm_roundtrip_nonce_12(&backend);
        }

        #[test]
        fn rustcrypto_gcm_nonce_16() {
            let backend = RustCryptoBackend;
            test_aes_gcm_roundtrip_nonce_16(&backend);
        }
    }
}
