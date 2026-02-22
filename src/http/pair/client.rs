use std::sync::Arc;

use uuid::Uuid;

use crate::http::{
    ClientIdentifier, ClientInfo, ClientSecret,
    pair::{HashAlgorithm, PairCryptoProvider, PairPin, PairRequest, PairResponse, SALT_LENGTH},
};

///
/// A polled output of [ClientPairing].
///
pub enum ClientPairingOutput {
    /// Send this request to the [super::PairEndpoint] and wait for the response.
    ///
    /// The response MUST then be passed into [ClientPairing::handle_response].
    SendRequest(PairRequest),
    /// Pairing failed
    // TODO: reason?
    Failure,
    /// Pairing to the server was successful.
    Success,
}

///
/// A sans io struct that will pair to a server using the given values.
///
/// ## Usage
///
/// ```
/// // TODO
/// ```
///
pub struct ClientPairing<Crypto> {
    client_uuid: Uuid,
    client_name: String,
    device_name: String,
    client_identifier: ClientIdentifier,
    client_secret: ClientSecret,
    crypto_provider: Crypto,
    state: State,
    waiting_request: bool,
}

enum State {
    Phase1,
    Phase2 {},
}

impl<Crypto> ClientPairing<Crypto>
where
    Crypto: PairCryptoProvider,
{
    pub fn new(
        client_info: ClientInfo,
        device_name: String,
        client_identifier: ClientIdentifier,
        client_secret: ClientSecret,
        crypto_provider: Crypto,
    ) -> Self {
        Self {
            client_uuid: client_info.uuid,
            client_name: client_info.unique_id.to_string(),
            device_name,
            client_identifier,
            client_secret,
            crypto_provider,
            state: State::Phase1,
            waiting_request: false,
        }
    }

    /// Handle the response after sending a request.
    pub fn handle_response(&mut self, response: &PairResponse) {
        todo!()
    }

    /// Poll for new actions or events.
    pub fn poll_output(&mut self) -> ClientPairingOutput {
        if self.waiting_request {
            panic!(
                "After a call to [ClientPairing::poll_output] [ClientPairing::handle_response] must be called. Please see the usage of ClientPairing."
            );
        }

        self.waiting_request = true;

        match self.state {
            State::Phase1 => {
                todo!()
            }
            _ => {
                todo!()
            }
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

fn hash_size_uneq(
    provider: impl PairCryptoProvider,
    algorithm: HashAlgorithm,
    data: &[u8],
    output: &mut [u8],
) {
    let mut hash = [0u8; HashAlgorithm::MAX_HASH_LEN];
    provider.hash(algorithm, data, &mut hash);

    output.copy_from_slice(&hash[0..output.len()]);
}

fn generate_aes_key(
    provider: impl PairCryptoProvider,
    algorithm: HashAlgorithm,
    salt: [u8; SALT_LENGTH],
    pin: PairPin,
) -> [u8; 16] {
    let mut hash = [0u8; 16];

    let salted = self::salt_pin(salt, pin);
    hash_size_uneq(provider, algorithm, &salted, &mut hash);

    hash
}

#[cfg(test)]
mod test {
    use std::str::FromStr;

    use pem::Pem;

    use crate::http::{
        ClientIdentifier, ClientInfo, ClientSecret, ServerIdentifier,
        pair::{
            HashAlgorithm, PairCryptoProvider,
            client::ClientPairing,
            test::{TEST_CLIENT_CERTIFICATE_PEM, TEST_CLIENT_PRIVATE_KEY_PEM},
        },
    };

    struct PanicCryptoProvider;

    impl PairCryptoProvider for PanicCryptoProvider {
        type Error = ();

        fn hash(&self, _algorithm: HashAlgorithm, _data: &[u8], _output: &mut [u8]) {
            unimplemented!()
        }
        fn decrypt_aes(&self, _key: &[u8], _ciphertext: &[u8]) -> Result<Vec<u8>, Self::Error> {
            unimplemented!()
        }
        fn encrypt_aes(&self, _key: &[u8], _plaintext: &[u8]) -> Result<Vec<u8>, Self::Error> {
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
    #[should_panic = "call to"]
    fn panic_on_double_poll_output() {
        let mut pairing = ClientPairing::new(
            ClientInfo::default(),
            "roth".to_string(),
            ClientIdentifier::from_pem(Pem::from_str(TEST_CLIENT_CERTIFICATE_PEM).unwrap()),
            ClientSecret::from_pem(Pem::from_str(TEST_CLIENT_PRIVATE_KEY_PEM).unwrap()),
            PanicCryptoProvider,
        );

        let _ = pairing.poll_output();
        let _ = pairing.poll_output();
    }

    fn test_pair_with(crypto: impl PairCryptoProvider) {
        let mut pairing = ClientPairing::new(
            ClientInfo::default(),
            "roth".to_string(),
            ClientIdentifier::from_pem(Pem::from_str(TEST_CLIENT_CERTIFICATE_PEM).unwrap()),
            ClientSecret::from_pem(Pem::from_str(TEST_CLIENT_PRIVATE_KEY_PEM).unwrap()),
            crypto,
        );

        todo!()
    }

    #[cfg(feature = "openssl")]
    #[test]
    fn openssl() {
        use crate::openssl::OpenSSLCryptoProvider;

        test_pair_with(OpenSSLCryptoProvider);
    }

    // TODO: test this impl for correctness
}
