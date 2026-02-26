use std::{fmt, str::FromStr};

use roxmltree::Document;

use crate::http::{
    ParseError, QueryBuilder, QueryBuilderError, QueryIter, QueryParam, Request, TextResponse,
    helper::{parse_xml_child_text, parse_xml_root_node},
    pair::parse_xml_child_paired,
};

#[derive(Debug, Clone, PartialEq)]
pub struct PairPhase2Request {
    pub device_name: String,
    pub encrypted_challenge: Vec<u8>,
}

impl Request for PairPhase2Request {
    fn append_query_params(
        &self,
        query_builder: &mut impl QueryBuilder,
    ) -> Result<(), QueryBuilderError> {
        query_builder.append(QueryParam {
            key: "devicename",
            value: &self.device_name,
        })?;
        query_builder.append(QueryParam {
            key: "updateState",
            value: "1",
        })?;

        let encrypted_challenge_str = hex::encode_upper(&self.encrypted_challenge);
        query_builder.append(QueryParam {
            key: "clientchallenge",
            value: &encrypted_challenge_str,
        })?;

        Ok(())
    }

    fn from_query_params<'a, Q>(query_iter: &mut Q) -> Result<Self, ()>
    where
        Q: QueryIter<'a>,
    {
        todo!()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PairPhase2Response {
    pub paired: bool,
    /// Encrypted response contains when unencrypted:
    /// 0..[HashAlgorithm::hash_len()](crate::http::pair::HashAlgorithm::hash_len()): The response hash
    /// [HashAlgorithm::hash_len()](crate::http::pair::HashAlgorithm::hash_len())..hash_len() + [CHALLENGE_LENGTH](crate::http::pair::CHALLENGE_LENGTH): The server challenge
    pub encrypted_response: Vec<u8>,
}

impl TextResponse for PairPhase2Response {
    fn serialize_into(&self, body_writer: &mut impl fmt::Write) -> fmt::Result {
        todo!()
    }
}

impl FromStr for PairPhase2Response {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let doc = Document::parse(s)?;
        let root = parse_xml_root_node(&doc)?;

        let paired = parse_xml_child_paired(root)?;

        let challenge_response_str = parse_xml_child_text(root, "challengeresponse")?;
        let challenge_response = hex::decode(challenge_response_str)?;

        Ok(PairPhase2Response {
            paired,
            encrypted_response: challenge_response,
        })
    }
}
