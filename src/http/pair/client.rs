use thiserror::Error;

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
    #[error("failed because of an unknown reason")]
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
        server_challenge: [u8; CHALLENGE_LENGTH],
    },
    SendPhase4 {},
    RecvPhase4 {},
    SendPhase5,
    RecvPhase5,
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

        Ok(Self {
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
    ///
    /// response is [None] if an error occured in that response.
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
                    self.state = Some(State::Error);
                    return Err(ClientPairingError::Failed);
                };

                if !response.paired {
                    self.state = Some(State::Error);
                    return Err(ClientPairingError::Failed);
                }

                let Some(server_certificate) = response.certificate else {
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
                    self.state = Some(State::Error);
                    return Err(ClientPairingError::Failed);
                };

                if !response.paired {
                    self.state = Some(State::Error);
                    return Err(ClientPairingError::Failed);
                }

                let response = self
                    .crypto_backend
                    .decrypt_aes(&self.aes_key, &response.encrypted_response)?;

                if response.len() > self.hash_algorithm.hash_len() + CHALLENGE_LENGTH {
                    self.state = Some(State::Error);
                    return Err(ClientPairingError::Failed);
                }

                let mut server_response_hash = [0; _];
                server_response_hash[0..self.hash_algorithm.hash_len()]
                    .copy_from_slice(&response[0..self.hash_algorithm.hash_len()]);

                let mut server_challenge = [0; _];
                server_challenge[0..CHALLENGE_LENGTH].copy_from_slice(
                    &response[self.hash_algorithm.hash_len()
                        ..(self.hash_algorithm.hash_len() + CHALLENGE_LENGTH)],
                );

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
                server_challenge,
            } => {
                let PairResponse::Phase3(response) = response else {
                    self.state = Some(State::Error);
                    return Err(ClientPairingError::Failed);
                };

                if !response.paired {
                    self.state = Some(State::Error);
                    return Err(ClientPairingError::Failed);
                }

                // Validate server response
                // TODO: check length
                let mut server_secret = [0; 16];
                server_secret.copy_from_slice(&response.server_pairing_secret[0..16]);

                let server_signature = &response.server_pairing_secret[0..16];

                if !self.crypto_backend.verify_signature(
                    &server_secret,
                    server_signature,
                    &server_certificate,
                )? {
                    // MITM likely, cancel here

                    self.state = Some(State::Error);
                    return Err(ClientPairingError::Failed);
                }

                let mut expected_response = Vec::new();
                expected_response.extend_from_slice(&challenge);
                expected_response
                    .extend_from_slice(&self.crypto_backend.signature(&server_certificate)?);
                expected_response.extend_from_slice(&server_secret);

                let mut expected_response_hash = [0; HashAlgorithm::MAX_HASH_LEN];
                hash_size_uneq(
                    &self.crypto_backend,
                    self.hash_algorithm,
                    &expected_response,
                    &mut expected_response_hash,
                )?;

                if server_response_hash != expected_response_hash {
                    self.state = Some(State::Error);
                    return Err(ClientPairingError::FailedWrongPin);
                }

                self.state = Some(State::SendPhase4 {});

                Ok(())
            }
            State::RecvPhase4 {} => {
                let PairResponse::Phase4(response) = response else {
                    self.state = Some(State::Error);
                    return Err(ClientPairingError::Failed);
                };

                if !response.paired {
                    self.state = Some(State::Error);
                    return Err(ClientPairingError::Failed);
                }

                self.state = Some(State::SendPhase5);

                Ok(())
            }
            State::RecvPhase5 => {
                let PairResponse::Phase5(response) = response else {
                    self.state = Some(State::Error);
                    return Err(ClientPairingError::Failed);
                };

                if !response.paired {
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
                challenge_response
                    .extend_from_slice(&self.crypto_backend.signature(&server_certificate)?);
                challenge_response.extend_from_slice(self.client_secret.to_pem().contents());

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
                    server_challenge,
                    server_response_hash,
                });

                Ok(ClientPairingOutput::SendHttpPairRequest(
                    PairRequest::Phase3(PairPhase3Request {
                        device_name: self.device_name.clone(),
                        encrypted_challenge_response_hash,
                    }),
                ))
            }
            State::SendPhase4 {} => {
                // Send the server out signed certificate
                let mut client_pairing_secret = Vec::new();
                client_pairing_secret.extend_from_slice(&self.secret);
                client_pairing_secret.extend_from_slice(
                    &self
                        .crypto_backend
                        .sign_data(&self.client_secret, &self.secret)?,
                );

                self.state = Some(State::RecvPhase4 {});

                Ok(ClientPairingOutput::SendHttpPairRequest(
                    PairRequest::Phase4(PairPhase4Request {
                        device_name: self.device_name.clone(),
                        client_pairing_secret,
                    }),
                ))
            }
            State::SendPhase5 => Ok(ClientPairingOutput::SendHttpsPairRequest(
                PairRequest::Phase5(PairPhase5Request {
                    device_name: self.device_name.clone(),
                }),
            )),
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

    let salted = self::salt_pin(salt, pin);
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
                phase5::{PairPhase5Request, PairPhase5Response},
                test::{
                    PAIR_CLIENT_CERTIFICATE_PEM, PAIR_CLIENT_PRIVATE_KEY_PEM,
                    PAIR_SERVER_CERTIFICATE_PEM,
                },
            },
        },
    };

    struct PanicCryptoProvider;
    #[derive(Debug, Error, PartialEq)]
    enum PanicError {}

    impl PairingCryptoBackend for PanicCryptoProvider {
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
            unimplemented!()
        }
        fn decrypt_aes(&self, _key: &[u8], _ciphertext: &[u8]) -> Result<Vec<u8>, Self::Error> {
            unimplemented!()
        }
        fn encrypt_aes(&self, _key: &[u8], _plaintext: &[u8]) -> Result<Vec<u8>, Self::Error> {
            unimplemented!()
        }
        fn signature(
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
        let mut pairing = ClientPairing::new(
            ClientIdentifier::from_pem(Pem::from_str(PAIR_CLIENT_CERTIFICATE_PEM).unwrap()),
            ClientSecret::from_pem(Pem::from_str(PAIR_CLIENT_PRIVATE_KEY_PEM).unwrap()),
            ServerVersion::new(7, 1, 143, -1),
            "roth".to_string(),
            PairPin::new(0, 0, 0, 0).unwrap(),
            PanicCryptoProvider,
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
        let pin = PairPin::new(9, 4, 9, 3).unwrap();
        let device_name = "roth".to_string();

        let challenge = [
            108, 93, 159, 29, 132, 223, 45, 18, 69, 127, 100, 195, 0, 242, 115, 228,
        ];
        let salt = *hex::decode("17CA4D60C3A445B67E45BD71933B6D7E")
            .unwrap()
            .as_array()
            .unwrap();
        // TODO: fix this pair_secret
        let client_pair_secret = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];

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
                encrypted_challenge: hex::decode("FCF9B608577AAE74F1A3FA02CF01128E").unwrap(),
            })),
        );

        pairing.handle_response(PairResponse::Phase2(PairPhase2Response {
            paired: true,
            encrypted_response: hex::decode("AEB1A10071A8FD93B1D62651103CBB971DCCE4432248858652CB2890F781B90EDDF5572014F6DEF6C7E337CBEB049CCC").unwrap(),
        })).unwrap();

        // Phase 3
        assert_eq!(
            pairing.poll_output().unwrap(),
            ClientPairingOutput::SendHttpPairRequest(PairRequest::Phase3(PairPhase3Request {
                device_name: device_name.to_string(),
                encrypted_challenge_response_hash: hex::decode(
                    "6ECA266A3E6CE115DB8B0951AC11C713DBB5EEB1EFD53DC6C9B7245E8DDB1026",
                )
                .unwrap(),
            }))
        );

        pairing.handle_response(PairResponse::Phase3(PairPhase3Response {
            paired: true,
            server_pairing_secret: hex::decode("2B350A80904FC1F7AFEE26F9D3775F620B2DD3E92B1E4D300D95C24579075D5CC238792FA24D367DC9DB040F9E4CB1F3819E17E28CC8941CDB37F149FFDA86E6A1B22042A98FAC594E07A05FFEC86BB8130461CB40BC075080801D501969C6D6B1D29AEA20CF01CB6DD478C349A488616D2755804283B3ACBEAF55101E91B6C1AC62C99D3CA224FC7ADBE692A32657C52D5675626B4A1EE9170EE0CA27D65F0C0FE05CE410EF88A3DA4DAAFE79E4DD51FE078E063B6926957CE1599DAEB9F167DA1B816140EBF3EFDA438F3DA1AE78F1596113E1F95CB954CA3FFF77A9816EC8E0D79A4F1996FA8D3BAE896498E30D9F104F1AA8969476A6820A085A1CB56480F327096ED62AF8DF8BF6269BA380C335").unwrap(),
        })).unwrap();

        // Phase 4
        assert_eq!(
            pairing.poll_output().unwrap(),
            ClientPairingOutput::SendHttpPairRequest(PairRequest::Phase4(PairPhase4Request {
                device_name: device_name.to_string(),
                client_pairing_secret: hex::decode(
                    "87720E7BCAA43F2AF6A25B8B010AA77439EF79EC1066A87C55F7EB2BA2C415B8D03068AC044F9A7D4D1203B97420A949A861B69EFA5BCD327A51A54C18ED3B4CE83BD15C363E9FC77C640DB630CE1A3C54A7E2D7933AF643A711175FFCC2CF1B683E98C84726477FB0E6954C13FB3DFAE81BDC00B60C10249BD21B795F3F8883F02E0D8863FBFEBA0273DE84A07D0A7F1F4BB41B84722BF17B8F26E9746E9623DEBA471C037BF87A5F83BABFFBAB30294336527E5F95A1AEC8E8FB59A0D50841C00E865F1C60EA7D3F7F6D98260CD57C512D9EABCDF1176D13C335320573B36B2B873CBFB8FB0B7FC4AE891BA4DE5F29151E817B210C98D6F9EC1E970AF0A9D755EB62A71A3D8FC6682BA6E411934D1C",
                ).unwrap(),
    })),
        );

        pairing
            .handle_response(PairResponse::Phase4(PairPhase4Response { paired: true }))
            .unwrap();

        // Phase 5
        assert_eq!(
            pairing.poll_output().unwrap(),
            ClientPairingOutput::SendHttpPairRequest(PairRequest::Phase5(PairPhase5Request {
                device_name: device_name.to_string(),
            })),
        );

        pairing
            .handle_response(PairResponse::Phase5(PairPhase5Response { paired: true }))
            .unwrap();

        // Success
        assert_eq!(pairing.poll_output().unwrap(), ClientPairingOutput::Success);
    }

    #[cfg(feature = "openssl")]
    #[test]
    fn pair_openssl() {
        use crate::crypto::openssl::OpenSSLCryptoBackend;

        test_pair_with(OpenSSLCryptoBackend);
    }
}
