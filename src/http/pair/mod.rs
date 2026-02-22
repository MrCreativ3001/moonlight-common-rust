//! Moonlilght Pairing
//!
//! References:
//! - https://games-on-whales.github.io/wolf/stable/protocols/http-pairing.html
//! - Moonlight-Embedded:

use std::{
    fmt::{self, Debug, Display},
    str::FromStr,
};

use roxmltree::Node;

use crate::{
    ServerVersion,
    http::{
        ClientSecret, Endpoint, ParseError, QueryBuilder, QueryBuilderError, QueryIter, Request,
        ServerIdentifier, TextResponse,
        helper::parse_xml_child_text,
        pair::{
            phase1::{PairPhase1Request, PairPhase2Response},
            phase2::{PairPhase1Response, PairPhase2Request},
            phase3::{PairPhase3Request, PairPhase3Response},
            phase4::{PairPhase4Request, PairPhase4Response},
            phase5::{PairPhase5Request, PairPhase5Response},
        },
    },
};

pub mod client;

pub mod phase1;
pub mod phase2;
pub mod phase3;
pub mod phase4;
pub mod phase5;

#[cfg(test)]
mod test;

/// A pin which contains four values in the range 0..10
#[derive(Clone, Copy)]
pub struct PairPin {
    numbers: [u8; 4],
}

impl PairPin {
    pub fn from_array(numbers: [u8; 4]) -> Option<Self> {
        let range = 0..10;

        if range.contains(&numbers[0])
            && range.contains(&numbers[1])
            && range.contains(&numbers[2])
            && range.contains(&numbers[3])
        {
            return Some(Self { numbers });
        }

        None
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
        todo!()
    }

    fn from_query_params<'a, Q>(query_iter: &mut Q) -> Result<Self, ()>
    where
        Q: QueryIter<'a>,
    {
        todo!()
    }
}

pub enum PairResponse {
    Phase1(PairPhase1Response),
    Phase2(PairPhase2Response),
    Phase3(PairPhase3Response),
    Phase4(PairPhase4Response),
    Phase5(PairPhase5Response),
}

impl TextResponse for PairResponse {
    fn serialize_into(&self, body_writer: &mut impl fmt::Write) -> fmt::Result {
        todo!()
    }
}

impl FromStr for PairResponse {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        todo!()
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

pub trait PairCryptoProvider {
    type Error;

    /// Hashes data into the output buffer provided.
    fn hash(&self, algorithm: HashAlgorithm, data: &[u8], output: &mut [u8]);

    /// Encrypts plaintext using aes 128 bit ecb with the provided key.
    fn encrypt_aes(&self, key: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, Self::Error>;

    /// Decrypts plaintext using aes 128 bit ecb with the provided key.
    fn decrypt_aes(&self, key: &[u8], ciphertext: &[u8]) -> Result<Vec<u8>, Self::Error>;

    /// Verifies the signature using sha256
    fn verify_signature(
        &self,
        server_secret: &[u8],
        server_signature: &[u8],
        server_cert: &ServerIdentifier,
    ) -> Result<bool, Self::Error>;

    /// Signs the data using sha256
    fn sign_data(&self, private_key: &ClientSecret, data: &[u8]) -> Result<Vec<u8>, Self::Error>;
}
