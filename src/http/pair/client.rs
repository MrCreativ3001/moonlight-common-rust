use thiserror::Error;
use tracing::{Level, Span, debug, debug_span, instrument};

use crate::{
    ServerVersion,
    http::{
        ClientIdentifier, ClientSecret, ServerIdentifier,
        pair::{
            CHALLENGE_LENGTH, HashAlgorithm, PairPin, PairRequest, PairResponse,
            PairingCryptoBackend, SALT_LENGTH, hash_algorithm_for_server,
            phase1::PairPhase1Request, phase2::PairPhase2Request, phase3::PairPhase3Request,
            phase4::PairPhase4Request, phase5::PairPhase5Request,
        },
    },
};

///
/// A polled output of [ClientPairing].
///
#[derive(Debug, PartialEq)]
pub enum ClientPairingOutput {
    /// Send this request over http to the [PairEndpoint](super::PairEndpoint) and wait for the response.
    ///
    /// The response MUST then be passed into [ClientPairing::handle_response].
    SendHttpPairRequest(PairRequest),
    /// Sets the [ServerIdentifier] for future https requests.
    ///
    /// After this is returned from the implementation https requests are allowed to be returned and all requests MUST have this identifier / certificate.
    /// If this is not the case cancel the connection because a MITM is likely happening.
    SetServerIdentifier(ServerIdentifier),
    /// Send this request over https to the [PairEndpoint](super::PairEndpoint) and wait for the response.
    /// The [ServerIdentifier] must be the https certificate of the server. If this is not the case the request MUST fail because a MITM is likely.
    ///
    /// The response MUST then be passed into [ClientPairing::handle_response].
    SendHttpsPairRequest(PairRequest),
    /// Pairing to the server was successful.
    /// The client identity and the server identifier from [ClientPairingOutput::SetServerIdentifier] can be used on the server to make authenticated https requests.
    ///
    /// The [ClientPairing] struct can now be dropped.
    Success,
}

#[derive(Debug, Error, PartialEq)]
pub enum ClientPairingError<CryptoError> {
    #[error("another device is currently pairing with the server")]
    FailedAlreadyInProgress,
    #[error("failed to pair because the pin was incorrect")]
    FailedWrongPin,
    #[error("failed")]
    Failed,
    #[error("crypto: {0}")]
    Crypto(#[from] CryptoError),
}

impl<Error> ClientPairingError<Error> {
    pub fn from_err<F>(value: ClientPairingError<F>) -> Self
    where
        Error: From<F>,
    {
        match value {
            ClientPairingError::Crypto(crypto) => ClientPairingError::Crypto(crypto.into()),
            ClientPairingError::Failed => ClientPairingError::Failed,
            ClientPairingError::FailedAlreadyInProgress => {
                ClientPairingError::FailedAlreadyInProgress
            }
            ClientPairingError::FailedWrongPin => ClientPairingError::FailedWrongPin,
        }
    }
}

const KEY_LENGTH: usize = 16;
const CLIENT_PAIR_SECRET_LENGTH: usize = 16;

///
/// A sans io struct that will pair to a server using the given values.
/// After any function returns an error the struct MUST NOT be used again and the [UnpairEndpoint](crate::http::unpair::UnpairEndpoint) must be called.
///
/// ## Usage
///
/// ```
/// // TODO
/// ```
///
pub struct ClientPairing<Crypto> {
    span: Span,
    client_identifier: ClientIdentifier,
    client_secret: ClientSecret,
    hash_algorithm: HashAlgorithm,
    device_name: String,
    salt: [u8; SALT_LENGTH],
    aes_key: [u8; KEY_LENGTH],
    secret: [u8; CLIENT_PAIR_SECRET_LENGTH],
    crypto_backend: Crypto,
    state: Option<State>,
}

#[derive(Debug)]
enum State {
    Error,
    SendPhase1 {
        challenge: [u8; CHALLENGE_LENGTH],
    },
    RecvPhase1 {
        challenge: [u8; CHALLENGE_LENGTH],
    },
    SendPhase2 {
        challenge: [u8; CHALLENGE_LENGTH],
        server_certificate: ServerIdentifier,
    },
    RecvPhase2 {
        challenge: [u8; CHALLENGE_LENGTH],
        server_certificate: ServerIdentifier,
    },
    SendPhase3 {
        challenge: [u8; CHALLENGE_LENGTH],
        server_certificate: ServerIdentifier,
        server_response_hash: [u8; HashAlgorithm::MAX_HASH_LEN],
        server_challenge: [u8; CHALLENGE_LENGTH],
    },
    RecvPhase3 {
        challenge: [u8; CHALLENGE_LENGTH],
        server_certificate: ServerIdentifier,
        server_response_hash: [u8; HashAlgorithm::MAX_HASH_LEN],
    },
    SendPhase4 {
        server_certificate: ServerIdentifier,
    },
    RecvPhase4 {
        server_certificate: ServerIdentifier,
    },
    SetCertificate {
        server_certificate: ServerIdentifier,
    },
    SendPhase5 {},
    RecvPhase5 {},
    Success,
}

impl<Crypto> ClientPairing<Crypto>
where
    Crypto: PairingCryptoBackend,
{
    pub fn new(
        client_identifier: ClientIdentifier,
        client_secret: ClientSecret,
        server_version: ServerVersion,
        device_name: String,
        pin: PairPin,
        crypto_provider: Crypto,
    ) -> Result<Self, ClientPairingError<Crypto::Error>> {
        let mut salt = [0; _];
        crypto_provider.random_bytes(&mut salt)?;

        let mut challenge = [0; _];
        crypto_provider.random_bytes(&mut challenge)?;

        let mut client_pair_secret = [0; _];
        crypto_provider.random_bytes(&mut client_pair_secret)?;

        Self::new_inner(
            client_identifier,
            client_secret,
            server_version,
            device_name,
            pin,
            salt,
            challenge,
            client_pair_secret,
            crypto_provider,
        )
    }
    pub(crate) fn new_inner(
        client_identifier: ClientIdentifier,
        client_secret: ClientSecret,
        server_version: ServerVersion,
        device_name: String,
        pin: PairPin,
        salt: [u8; SALT_LENGTH],
        challenge: [u8; CHALLENGE_LENGTH],
        client_pair_secret: [u8; CLIENT_PAIR_SECRET_LENGTH],
        crypto_provider: Crypto,
    ) -> Result<Self, ClientPairingError<Crypto::Error>> {
        let hash_algorithm = hash_algorithm_for_server(server_version);
        let aes_key = generate_aes_key(&crypto_provider, hash_algorithm, salt, pin)?;

        let span = debug_span!("moonlight::client::pairing");

        Ok(Self {
            span,
            client_identifier,
            client_secret,
            device_name,
            hash_algorithm,
            salt,
            aes_key,
            secret: client_pair_secret,
            state: Some(State::SendPhase1 { challenge }),
            crypto_backend: crypto_provider,
        })
    }

    /// Handle the response after sending a request.
    #[instrument(level = Level::DEBUG, parent = &self.span, fields(state = ?&self.state), skip(self), ret, err)]
    pub fn handle_response(
        &mut self,
        response: PairResponse,
    ) -> Result<(), ClientPairingError<Crypto::Error>> {
        let state = self
            .state
            .take()
            .expect("no state found inside of ClientPairing, this is a bug!");

        match state {
            State::Error => {
                panic!(
                    "The ClientPairing implementation already errored. It cannot be used again!"
                );
            }
            State::RecvPhase1 { challenge } => {
                let PairResponse::Phase1(response) = response else {
                    debug!(reason = "wrong response", "pairing failed");

                    self.state = Some(State::Error);
                    return Err(ClientPairingError::Failed);
                };

                if !response.paired {
                    debug!(reason = "response.paired = false", "pairing failed");

                    self.state = Some(State::Error);
                    return Err(ClientPairingError::Failed);
                }

                let Some(server_certificate) = response.certificate else {
                    debug!(reason = "no certificate", "pairing failed");

                    self.state = Some(State::Error);
                    return Err(ClientPairingError::Failed);
                };

                self.state = Some(State::SendPhase2 {
                    challenge,
                    server_certificate: ServerIdentifier::from_pem(server_certificate),
                });

                Ok(())
            }
            State::RecvPhase2 {
                challenge,
                server_certificate,
            } => {
                let PairResponse::Phase2(response) = response else {
                    debug!(reason = "wrong response", "pairing failed");

                    self.state = Some(State::Error);
                    return Err(ClientPairingError::Failed);
                };

                if !response.paired {
                    debug!(reason = "response.paired = false", "pairing failed");

                    self.state = Some(State::Error);
                    return Err(ClientPairingError::Failed);
                }

                let response = self
                    .crypto_backend
                    .decrypt_aes(&self.aes_key, &response.encrypted_response)?;

                let required_len = self.hash_algorithm.hash_len() + CHALLENGE_LENGTH;
                if response.len() < required_len {
                    debug!(
                        reason = "response is smaller than expected",
                        len_got = response.len(),
                        len_expected = required_len,
                        "pairing failed"
                    );

                    self.state = Some(State::Error);
                    return Err(ClientPairingError::Failed);
                }

                let hash_len = self.hash_algorithm.hash_len();

                let mut server_response_hash = [0; _];
                server_response_hash[0..hash_len].copy_from_slice(&response[0..hash_len]);

                let mut server_challenge = [0; _];
                server_challenge[0..CHALLENGE_LENGTH]
                    .copy_from_slice(&response[hash_len..(hash_len + CHALLENGE_LENGTH)]);

                self.state = Some(State::SendPhase3 {
                    challenge,
                    server_certificate,
                    server_response_hash,
                    server_challenge,
                });

                Ok(())
            }
            State::RecvPhase3 {
                challenge,
                server_certificate,
                server_response_hash,
            } => {
                let PairResponse::Phase3(response) = response else {
                    debug!(reason = "wrong response", "pairing failed");

                    self.state = Some(State::Error);
                    return Err(ClientPairingError::Failed);
                };

                if !response.paired {
                    debug!(reason = "response.paired = false", "pairing failed");

                    self.state = Some(State::Error);
                    return Err(ClientPairingError::Failed);
                }

                // Validate server response
                let expected_len = 16;
                if response.server_pairing_secret.len() < expected_len {
                    debug!(
                        reason = "response.server_pairing_secret too short",
                        got_len = response.server_pairing_secret.len(),
                        expected_len = expected_len,
                        "pairing failed"
                    );

                    self.state = Some(State::Error);
                    return Err(ClientPairingError::Failed);
                }

                let mut server_secret = [0; 16];
                server_secret.copy_from_slice(&response.server_pairing_secret[0..16]);

                let server_signature = &response.server_pairing_secret[16..];

                if !self.crypto_backend.verify_signature(
                    &server_secret,
                    server_signature,
                    &server_certificate,
                )? {
                    debug!(
                        reason = "verify signature failed -> MITM likely -> cancelling pairing",
                        "pairing failed"
                    );

                    // MITM likely, cancel here

                    self.state = Some(State::Error);
                    return Err(ClientPairingError::Failed);
                }

                let mut expected_response = Vec::new();
                expected_response.extend_from_slice(&challenge);
                expected_response
                    .extend_from_slice(&self.crypto_backend.server_signature(&server_certificate)?);
                expected_response.extend_from_slice(&server_secret);

                let mut expected_response_hash = [0; HashAlgorithm::MAX_HASH_LEN];
                hash_size_uneq(
                    &self.crypto_backend,
                    self.hash_algorithm,
                    &expected_response,
                    &mut expected_response_hash,
                )?;

                if server_response_hash != expected_response_hash {
                    debug!(
                        reason = "server_response_hash != expected_response_hash, user likely entered wrong pin",
                        "pairing failed"
                    );

                    self.state = Some(State::Error);
                    return Err(ClientPairingError::FailedWrongPin);
                }

                self.state = Some(State::SendPhase4 { server_certificate });

                Ok(())
            }
            State::RecvPhase4 { server_certificate } => {
                let PairResponse::Phase4(response) = response else {
                    debug!(reason = "wrong response", "pairing failed");

                    self.state = Some(State::Error);
                    return Err(ClientPairingError::Failed);
                };

                if !response.paired {
                    debug!(reason = "response.paired = false", "pairing failed");

                    self.state = Some(State::Error);
                    return Err(ClientPairingError::Failed);
                }

                self.state = Some(State::SetCertificate { server_certificate });

                Ok(())
            }
            State::RecvPhase5 {} => {
                let PairResponse::Phase4(response) = response else {
                    debug!(reason = "wrong response", "pairing failed");

                    self.state = Some(State::Error);
                    return Err(ClientPairingError::Failed);
                };

                if !response.paired {
                    debug!(reason = "response.paired = false", "pairing failed");

                    self.state = Some(State::Error);
                    return Err(ClientPairingError::Failed);
                }

                self.state = Some(State::Success);

                Ok(())
            }
            _ => panic!("A call to [ClientPairing::poll_output] was expected!"),
        }
    }

    /// Poll for new actions or events.
    ///
    /// If this returns [ClientPairingOutput::SetServerIdentifier] you MUST poll this function again, without calling [ClientPairing::handle_response]
    #[instrument(level = Level::DEBUG, parent = &self.span, fields(state = ?&self.state), skip(self), ret, err)]
    pub fn poll_output(
        &mut self,
    ) -> Result<ClientPairingOutput, ClientPairingError<Crypto::Error>> {
        let state = self
            .state
            .take()
            .expect("no state found inside of ClientPairing, this is a bug!");

        match state {
            State::Error => {
                panic!(
                    "The ClientPairing implementation already errored. It cannot be used again!"
                );
            }
            State::SendPhase1 { challenge } => {
                self.state = Some(State::RecvPhase1 { challenge });

                Ok(ClientPairingOutput::SendHttpPairRequest(
                    PairRequest::Phase1(PairPhase1Request {
                        client_certificate: self.client_identifier.to_pem(),
                        device_name: self.device_name.clone(),
                        salt: self.salt,
                    }),
                ))
            }
            State::SendPhase2 {
                challenge,
                server_certificate,
            } => {
                let encrypted_challenge =
                    self.crypto_backend.encrypt_aes(&self.aes_key, &challenge)?;

                self.state = Some(State::RecvPhase2 {
                    challenge,
                    server_certificate,
                });

                Ok(ClientPairingOutput::SendHttpPairRequest(
                    PairRequest::Phase2(PairPhase2Request {
                        device_name: self.device_name.to_string(),
                        encrypted_challenge,
                    }),
                ))
            }
            State::SendPhase3 {
                challenge,
                server_certificate,
                server_response_hash,
                server_challenge,
            } => {
                let mut challenge_response = Vec::new();
                challenge_response.extend_from_slice(&server_challenge);
                challenge_response.extend_from_slice(
                    &self
                        .crypto_backend
                        .client_signature(&self.client_identifier)?,
                );
                challenge_response.extend_from_slice(&self.secret);

                let mut challenge_response_hash = [0; HashAlgorithm::MAX_HASH_LEN];
                hash_size_uneq(
                    &self.crypto_backend,
                    self.hash_algorithm,
                    &challenge_response,
                    &mut challenge_response_hash[0..self.hash_algorithm.hash_len()],
                )?;

                let encrypted_challenge_response_hash = self.crypto_backend.encrypt_aes(
                    &self.aes_key,
                    &challenge_response_hash[0..self.hash_algorithm.hash_len()],
                )?;

                self.state = Some(State::RecvPhase3 {
                    challenge,
                    server_certificate,
                    server_response_hash,
                });

                Ok(ClientPairingOutput::SendHttpPairRequest(
                    PairRequest::Phase3(PairPhase3Request {
                        device_name: self.device_name.clone(),
                        encrypted_challenge_response_hash,
                    }),
                ))
            }
            State::SendPhase4 { server_certificate } => {
                // Send the server out signed certificate
                let mut client_pairing_secret = Vec::new();
                client_pairing_secret.extend_from_slice(&self.secret);
                client_pairing_secret.extend_from_slice(
                    &self
                        .crypto_backend
                        .sign_data(&self.client_secret, &self.secret)?,
                );

                self.state = Some(State::RecvPhase4 { server_certificate });

                Ok(ClientPairingOutput::SendHttpPairRequest(
                    PairRequest::Phase4(PairPhase4Request {
                        device_name: self.device_name.clone(),
                        client_pairing_secret,
                    }),
                ))
            }
            State::SetCertificate { server_certificate } => {
                self.state = Some(State::SendPhase5 {});

                Ok(ClientPairingOutput::SetServerIdentifier(server_certificate))
            }
            State::SendPhase5 {} => {
                self.state = Some(State::RecvPhase5 {});

                Ok(ClientPairingOutput::SendHttpsPairRequest(
                    PairRequest::Phase5(PairPhase5Request {
                        device_name: self.device_name.clone(),
                    }),
                ))
            }
            State::Success => Ok(ClientPairingOutput::Success),
            _ => panic!(
                "After a call to [ClientPairing::poll_output] [ClientPairing::handle_response] must be called. Please see the usage of ClientPairing."
            ),
        }
    }
}

fn salt_pin(salt: [u8; SALT_LENGTH], pin: PairPin) -> [u8; SALT_LENGTH + 4] {
    let mut out = [0u8; SALT_LENGTH + 4];

    out[0..16].copy_from_slice(&salt);

    let pin_utf8 = pin
        .array()
        .map(|value| char::from_digit(value as u32, 10).expect("a pin digit between 0-9") as u8);

    out[16..].copy_from_slice(&pin_utf8);

    out
}

fn hash_size_uneq<C>(
    provider: &C,
    algorithm: HashAlgorithm,
    data: &[u8],
    output: &mut [u8],
) -> Result<(), ClientPairingError<C::Error>>
where
    C: PairingCryptoBackend,
{
    let mut hash = [0u8; HashAlgorithm::MAX_HASH_LEN];
    provider.hash(algorithm, data, &mut hash)?;

    output.copy_from_slice(&hash[0..output.len()]);

    Ok(())
}

fn generate_aes_key<C>(
    provider: &C,
    algorithm: HashAlgorithm,
    salt: [u8; SALT_LENGTH],
    pin: PairPin,
) -> Result<[u8; KEY_LENGTH], ClientPairingError<C::Error>>
where
    C: PairingCryptoBackend,
{
    let mut hash = [0u8; KEY_LENGTH];

    let salted = salt_pin(salt, pin);
    hash_size_uneq(provider, algorithm, &salted, &mut hash)?;

    Ok(hash)
}

#[allow(clippy::unwrap_used)]
#[cfg(test)]
mod test {
    use std::{fmt::Debug, str::FromStr};

    use pem::Pem;
    use thiserror::Error;

    use crate::{
        ServerVersion,
        http::{
            ClientIdentifier, ClientSecret, ServerIdentifier,
            pair::{
                HashAlgorithm, PairPin, PairRequest, PairResponse, PairingCryptoBackend,
                client::{ClientPairing, ClientPairingError, ClientPairingOutput},
                phase1::{PairPhase1Request, PairPhase1Response},
                phase2::{PairPhase2Request, PairPhase2Response},
                phase3::{PairPhase3Request, PairPhase3Response},
                phase4::{PairPhase4Request, PairPhase4Response},
                phase5::PairPhase5Request,
                test::{
                    PAIR_CLIENT_CERTIFICATE_PEM, PAIR_CLIENT_PRIVATE_KEY_PEM,
                    PAIR_SERVER_CERTIFICATE_PEM,
                },
            },
        },
        init_test,
        test::init_test,
    };

    struct TestCryptoProvider;
    #[derive(Debug, Error, PartialEq)]
    enum PanicError {}

    impl PairingCryptoBackend for TestCryptoProvider {
        type Error = PanicError;

        fn generate_client_identity(
            &self,
        ) -> Result<(ClientIdentifier, ClientSecret), Self::Error> {
            unimplemented!()
        }

        fn hash(
            &self,
            _algorithm: HashAlgorithm,
            _data: &[u8],
            _output: &mut [u8],
        ) -> Result<(), Self::Error> {
            unimplemented!()
        }
        fn random_bytes(&self, _data: &mut [u8]) -> Result<(), Self::Error> {
            Ok(())
        }
        fn decrypt_aes(&self, _key: &[u8], _ciphertext: &[u8]) -> Result<Vec<u8>, Self::Error> {
            unimplemented!()
        }
        fn encrypt_aes(&self, _key: &[u8], _plaintext: &[u8]) -> Result<Vec<u8>, Self::Error> {
            unimplemented!()
        }
        fn client_signature(
            &self,
            _client_certificate: &ClientIdentifier,
        ) -> Result<Vec<u8>, Self::Error> {
            unimplemented!()
        }
        fn server_signature(
            &self,
            _server_certificate: &ServerIdentifier,
        ) -> Result<Vec<u8>, Self::Error> {
            unimplemented!()
        }
        fn sign_data(
            &self,
            _private_key: &ClientSecret,
            _data: &[u8],
        ) -> Result<Vec<u8>, Self::Error> {
            unimplemented!()
        }
        fn verify_signature(
            &self,
            _server_secret: &[u8],
            _server_signature: &[u8],
            _server_cert: &ServerIdentifier,
        ) -> Result<bool, Self::Error> {
            unimplemented!()
        }
    }

    #[test]
    fn pair_already_in_progress() {
        init_test();

        let mut pairing = ClientPairing::new(
            ClientIdentifier::from_pem(Pem::from_str(PAIR_CLIENT_CERTIFICATE_PEM).unwrap()),
            ClientSecret::from_pem(Pem::from_str(PAIR_CLIENT_PRIVATE_KEY_PEM).unwrap()),
            ServerVersion::new(7, 1, 143, -1),
            "roth".to_string(),
            PairPin::new(0, 0, 0, 0).unwrap(),
            TestCryptoProvider,
        )
        .unwrap();

        // Phase 1
        let _ = pairing.poll_output();

        assert_eq!(
            pairing.handle_response(PairResponse::Phase1(PairPhase1Response {
                paired: true,
                certificate: None
            })),
            Err(ClientPairingError::FailedAlreadyInProgress),
        );
    }

    fn test_pair_with<C>(crypto: C)
    where
        C: PairingCryptoBackend,
        C::Error: Debug,
    {
        let pin = PairPin::new(6, 0, 0, 2).unwrap();
        let device_name = "roth".to_string();

        let challenge = [
            255, 100, 238, 159, 252, 80, 98, 231, 40, 13, 124, 105, 196, 106, 151, 173,
        ];
        let salt = [
            130, 128, 118, 7, 221, 223, 88, 215, 115, 12, 225, 224, 23, 37, 189, 189,
        ];
        let client_pair_secret = [
            87, 250, 40, 120, 155, 247, 206, 105, 245, 223, 62, 84, 243, 5, 108, 219,
        ];

        let mut pairing = ClientPairing::new_inner(
            ClientIdentifier::from_pem(Pem::from_str(PAIR_CLIENT_CERTIFICATE_PEM).unwrap()),
            ClientSecret::from_pem(Pem::from_str(PAIR_CLIENT_PRIVATE_KEY_PEM).unwrap()),
            ServerVersion::new(7, 1, 143, -1),
            device_name.clone(),
            pin,
            salt,
            challenge,
            client_pair_secret,
            crypto,
        )
        .unwrap();

        // See the [crate::http::test] file for the exact requests that are simulated here

        // Phase 1
        assert_eq!(
            pairing.poll_output().unwrap(),
            ClientPairingOutput::SendHttpPairRequest(PairRequest::Phase1(PairPhase1Request {
                device_name: device_name.clone(),
                client_certificate: Pem::from_str(PAIR_CLIENT_CERTIFICATE_PEM).unwrap(),
                salt,
            })),
        );

        pairing
            .handle_response(PairResponse::Phase1(PairPhase1Response {
                paired: true,
                certificate: Some(Pem::from_str(PAIR_SERVER_CERTIFICATE_PEM).unwrap()),
            }))
            .unwrap();

        // Phase 2
        assert_eq!(
            pairing.poll_output().unwrap(),
            ClientPairingOutput::SendHttpPairRequest(PairRequest::Phase2(PairPhase2Request {
                device_name: device_name.clone(),
                encrypted_challenge: hex::decode("97A0935E210C8C34AC35EA42FECFE6B8").unwrap(),
            })),
        );

        pairing.handle_response(PairResponse::Phase2(PairPhase2Response {
            paired: true,
            encrypted_response: hex::decode("EFAB7A5E54EC2703D13EC83D0D3ADADBC4F4FD4C941EE8FFB3B426BC5D959EC53C7372885AFAACA0660AF413C924DE83").unwrap(),
        })).unwrap();

        // Phase 3
        assert_eq!(
            pairing.poll_output().unwrap(),
            ClientPairingOutput::SendHttpPairRequest(PairRequest::Phase3(PairPhase3Request {
                device_name: device_name.to_string(),
                encrypted_challenge_response_hash: hex::decode(
                    "E21701F44FED0F539053799A50DFDE1073E30F2541FDEADFA22703941948501F",
                )
                .unwrap(),
            }))
        );

        pairing.handle_response(PairResponse::Phase3(PairPhase3Response {
            paired: true,
            server_pairing_secret: hex::decode("B2A62490129E15563B30DB692122105239DF685779A1A42951C9E7D27C786391E9D1A4E24729B63DC594D18AB66377F9234D4F5266478101C599FDF4B3EE9CFDE5855CAF7339E09103A03A1C39EC86FB14FD31DFA2D3F6C8B2B87D5A08183152BCDE9396046B3646391B3789D5CAAEC49B1329E6D4AEF3DAFFC97D756AB4DDF72D78F6E672772A4C488F6D12DF480971CA66FEA2771C09055C2F4070732005E27B583A2032FBD40EF8037034C25713F95E0DB5422D9C4EFF2274A6324CF056F255B64416F0856384A1E0948CE444AC9FB417C2443286245C40904E59B5EF018EC472218D68C4D7F6E0DAF4DA88539D6E52BEF8E9E8332CEFC8B72697D89D7D3CA8A14092147C0F3A9ECB5287D65B5840").unwrap(),
        })).unwrap();

        // Phase 4
        assert_eq!(
            pairing.poll_output().unwrap(),
            ClientPairingOutput::SendHttpPairRequest(PairRequest::Phase4(PairPhase4Request {
                device_name: device_name.to_string(),
                client_pairing_secret: hex::decode(
                    "57FA28789BF7CE69F5DF3E54F3056CDB221C46E0892C5147D9D9B17A29ED4B35ED746CAD0B9789BA9D05A7B121B44C25461366CF1FDD2A319DE946B93F3C2AC0A16C9F88B44BF29FD52E6BA94536315D5016CB9A3CD330854BF00C58D544E765603ED7D262051B0E575487A40BB7CE404E5B9F344E180908FDA5C7C31B643403057805C979A4D12D7B9B88ABE94C0A11605120BA46F8F1FF12097C30373EC23224A91E39B2864D5503F6E1641012467BC5452F82B736D208D5DD92BF16AF7F37058E5BCA272F1B3D35EDD490B969BF9170CE3E8F2B86619799F08DF5657E16403656FA15510F3BF0209B95EEE85682515756DBAB758458CFDD84B9EC95656D050655763DA5C7D9FF1DA19E153E12BEE7",
                ).unwrap(),
    })),
        );

        pairing
            .handle_response(PairResponse::Phase4(PairPhase4Response { paired: true }))
            .unwrap();

        // Before phase 5 we need to set the server certificate
        assert_eq!(
            pairing.poll_output().unwrap(),
            ClientPairingOutput::SetServerIdentifier(ServerIdentifier::from_pem(
                Pem::from_str(PAIR_SERVER_CERTIFICATE_PEM).unwrap()
            ))
        );

        // Phase 5
        assert_eq!(
            pairing.poll_output().unwrap(),
            ClientPairingOutput::SendHttpsPairRequest(PairRequest::Phase5(PairPhase5Request {
                device_name: device_name.to_string(),
            })),
        );

        pairing
            .handle_response(PairResponse::Phase4(PairPhase4Response { paired: true }))
            .unwrap();

        // Success
        assert_eq!(pairing.poll_output().unwrap(), ClientPairingOutput::Success);
    }

    #[cfg(feature = "openssl")]
    #[test]
    fn pair_openssl() {
        use crate::crypto::openssl::OpenSSLCryptoBackend;

        init_test!();

        test_pair_with(OpenSSLCryptoBackend);
    }

    #[cfg(feature = "rustcrypto")]
    #[test]
    fn pair_rustcrypto() {
        use crate::crypto::rustcrypto::RustCryptoBackend;

        init_test!();

        test_pair_with(RustCryptoBackend);
    }
}
