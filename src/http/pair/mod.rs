//! Moonlilght Pairing
//!
//! References:
//! - https://games-on-whales.github.io/wolf/stable/protocols/http-pairing.html
//! - Moonlight-Embedded: https://github.com/moonlight-stream/moonlight-embedded/blob/master/libgamestream/client.c#L426

use std::{
    fmt::{self, Debug, Display},
    str::FromStr,
    sync::Arc,
};

use roxmltree::Node;

use crate::{
    ServerVersion,
    http::{
        ClientIdentifier, ClientSecret, Endpoint, ParseError, QueryBuilder, QueryBuilderError,
        QueryIter, Request, ServerIdentifier, TextResponse,
        helper::parse_xml_child_text,
        pair::{
            phase1::{PairPhase1Request, PairPhase1Response},
            phase2::{PairPhase2Request, PairPhase2Response},
            phase3::{PairPhase3Request, PairPhase3Response},
            phase4::{PairPhase4Request, PairPhase4Response},
            phase5::PairPhase5Request,
        },
    },
};

pub mod phase1;
pub mod phase2;
pub mod phase3;
pub mod phase4;
pub mod phase5;

pub mod client;

#[cfg(test)]
mod test;

/// A pin which contains four values in the range 0..10
#[derive(Clone, Copy)]
pub struct PairPin {
    numbers: [u8; 4],
}

impl PairPin {
    pub fn new_random<Crypto>(crypto_backend: &Crypto) -> Result<Self, Crypto::Error>
    where
        Crypto: PairingCryptoBackend,
    {
        let mut random = [0; 4];
        crypto_backend.random_bytes(&mut random)?;

        // The values must be inside of the range
        #[allow(clippy::unwrap_used)]
        let pin = PairPin::new(
            random[0] % 10,
            random[1] % 10,
            random[2] % 10,
            random[3] % 10,
        )
        .unwrap();

        Ok(pin)
    }

    pub fn new(n1: u8, n2: u8, n3: u8, n4: u8) -> Option<Self> {
        let range = 0..10;

        if range.contains(&n1) && range.contains(&n2) && range.contains(&n3) && range.contains(&n4)
        {
            return Some(Self {
                numbers: [n1, n2, n3, n4],
            });
        }

        None
    }

    pub fn from_array(numbers: [u8; 4]) -> Option<Self> {
        Self::new(numbers[0], numbers[1], numbers[2], numbers[3])
    }

    pub fn n(&self, index: usize) -> Option<u8> {
        self.numbers.get(index).copied()
    }
    pub fn n1(&self) -> u8 {
        self.numbers[0]
    }
    pub fn n2(&self) -> u8 {
        self.numbers[1]
    }
    pub fn n3(&self) -> u8 {
        self.numbers[2]
    }
    pub fn n4(&self) -> u8 {
        self.numbers[3]
    }

    pub fn array(&self) -> [u8; 4] {
        self.numbers
    }
}

impl Display for PairPin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}{}{}", self.n1(), self.n2(), self.n3(), self.n4())
    }
}
impl Debug for PairPin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PairPin(")?;
        Display::fmt(&self, f)?;
        write!(f, ")")?;

        Ok(())
    }
}

pub const SALT_LENGTH: usize = 16;
pub const CHALLENGE_LENGTH: usize = 16;

///
/// The [Endpoint] used for pairing.
///
/// This endpoint will be called multiple times from the same client for pairing in different phases.
///
/// The last request (pairing phase 5) MUST be made over https in order to make sure that the certificate can make https requests.
///
/// References:
/// - Wolf: https://games-on-whales.github.io/wolf/stable/protocols/http-pairing.html
pub struct PairEndpoint;

impl Endpoint for PairEndpoint {
    type Request = PairRequest;
    type Response = PairResponse;

    fn https_required() -> bool {
        false
    }

    fn path() -> &'static str {
        "/pair"
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum PairRequest {
    Phase1(PairPhase1Request),
    Phase2(PairPhase2Request),
    Phase3(PairPhase3Request),
    Phase4(PairPhase4Request),
    Phase5(PairPhase5Request),
}

impl Request for PairRequest {
    fn append_query_params(
        &self,
        query_builder: &mut impl QueryBuilder,
    ) -> Result<(), QueryBuilderError> {
        match self {
            PairRequest::Phase1(request) => request.append_query_params(query_builder),
            PairRequest::Phase2(request) => request.append_query_params(query_builder),
            PairRequest::Phase3(request) => request.append_query_params(query_builder),
            PairRequest::Phase4(request) => request.append_query_params(query_builder),
            PairRequest::Phase5(request) => request.append_query_params(query_builder),
        }
    }

    fn from_query_params<'a, Q>(query_iter: &mut Q) -> Result<Self, ()>
    where
        Q: QueryIter<'a>,
    {
        todo!()
    }
}

#[derive(Debug)]
pub enum PairResponse {
    Phase1(PairPhase1Response),
    Phase2(PairPhase2Response),
    Phase3(PairPhase3Response),
    /// Phase 5 is also Phase 4 because both have the same structure and data
    Phase4(PairPhase4Response),
}

impl TextResponse for PairResponse {
    fn serialize_into(&self, body_writer: &mut impl fmt::Write) -> fmt::Result {
        todo!()
    }
}

impl FromStr for PairResponse {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // TODO: find a better way to do this

        if s.contains("plaincert") {
            PairPhase1Response::from_str(s).map(Self::Phase1)
        } else if s.contains("challengeresponse") {
            PairPhase2Response::from_str(s).map(Self::Phase2)
        } else if s.contains("pairingsecret") {
            PairPhase3Response::from_str(s).map(Self::Phase3)
        } else {
            PairPhase4Response::from_str(s).map(Self::Phase4)
        }
    }
}

fn parse_xml_child_paired<'doc, 'node>(list_node: Node<'node, 'doc>) -> Result<bool, ParseError> {
    let paired: i32 = parse_xml_child_text(list_node, "paired")?.parse()?;
    Ok(paired == 1)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HashAlgorithm {
    Sha1,
    Sha256,
}

impl HashAlgorithm {
    pub const MAX_HASH_LEN: usize = 32;

    pub fn hash_len(&self) -> usize {
        match self {
            Self::Sha1 => 20,
            Self::Sha256 => 32,
        }
    }
}

fn hash_algorithm_for_server(server_version: ServerVersion) -> HashAlgorithm {
    if server_version.major >= 7 {
        HashAlgorithm::Sha256
    } else {
        HashAlgorithm::Sha1
    }
}

pub trait PairingCryptoBackend {
    type Error: std::error::Error;

    fn generate_client_identity(&self) -> Result<(ClientIdentifier, ClientSecret), Self::Error>;

    /// Hashes data into the output buffer provided.
    fn hash(
        &self,
        algorithm: HashAlgorithm,
        data: &[u8],
        output: &mut [u8],
    ) -> Result<(), Self::Error>;

    /// Puts random bytes into data.
    fn random_bytes(&self, data: &mut [u8]) -> Result<(), Self::Error>;

    /// Encrypts plaintext using aes 128 bit ecb with the provided key.
    fn encrypt_aes(&self, key: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, Self::Error>;

    /// Decrypts plaintext using aes 128 bit ecb with the provided key.
    fn decrypt_aes(&self, key: &[u8], ciphertext: &[u8]) -> Result<Vec<u8>, Self::Error>;

    fn client_signature(
        &self,
        client_certificate: &ClientIdentifier,
    ) -> Result<Vec<u8>, Self::Error>;
    fn server_signature(
        &self,
        server_certificate: &ServerIdentifier,
    ) -> Result<Vec<u8>, Self::Error>;

    /// Verifies the signature using sha256
    fn verify_signature(
        &self,
        server_secret: &[u8],
        server_signature: &[u8],
        server_certificate: &ServerIdentifier,
    ) -> Result<bool, Self::Error>;

    /// Signs the data using sha256
    fn sign_data(&self, private_key: &ClientSecret, data: &[u8]) -> Result<Vec<u8>, Self::Error>;
}

impl<T> PairingCryptoBackend for Arc<T>
where
    T: PairingCryptoBackend,
{
    type Error = T::Error;

    fn generate_client_identity(&self) -> Result<(ClientIdentifier, ClientSecret), Self::Error> {
        T::generate_client_identity(self)
    }

    fn decrypt_aes(&self, key: &[u8], ciphertext: &[u8]) -> Result<Vec<u8>, Self::Error> {
        T::decrypt_aes(self, key, ciphertext)
    }

    fn encrypt_aes(&self, key: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, Self::Error> {
        T::encrypt_aes(self, key, plaintext)
    }

    fn hash(
        &self,
        algorithm: HashAlgorithm,
        data: &[u8],
        output: &mut [u8],
    ) -> Result<(), Self::Error> {
        T::hash(self, algorithm, data, output)
    }

    fn random_bytes(&self, data: &mut [u8]) -> Result<(), Self::Error> {
        T::random_bytes(self, data)
    }

    fn sign_data(&self, private_key: &ClientSecret, data: &[u8]) -> Result<Vec<u8>, Self::Error> {
        T::sign_data(self, private_key, data)
    }

    fn client_signature(
        &self,
        client_certificate: &ClientIdentifier,
    ) -> Result<Vec<u8>, Self::Error> {
        T::client_signature(self, client_certificate)
    }
    fn server_signature(
        &self,
        server_certificate: &ServerIdentifier,
    ) -> Result<Vec<u8>, Self::Error> {
        T::server_signature(self, server_certificate)
    }

    fn verify_signature(
        &self,
        server_secret: &[u8],
        server_signature: &[u8],
        server_cert: &ServerIdentifier,
    ) -> Result<bool, Self::Error> {
        T::verify_signature(self, server_secret, server_signature, server_cert)
    }
}
