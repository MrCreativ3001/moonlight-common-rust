use std::{fmt, str::FromStr};

use roxmltree::Document;

use crate::http::{
    ParseError, QueryBuilder, QueryBuilderError, QueryIter, QueryParam, Request, TextResponse,
    helper::{parse_xml_child_text, parse_xml_root_node},
    pair::parse_xml_child_paired,
};

#[derive(Debug, Clone, PartialEq)]
pub struct PairPhase3Request {
    pub device_name: String,
    pub encrypted_challenge_response_hash: Vec<u8>,
}

impl Request for PairPhase3Request {
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

        let encrypted_challenge_str = hex::encode_upper(&self.encrypted_challenge_response_hash);
        query_builder.append(QueryParam {
            key: "serverchallengeresp",
            value: &encrypted_challenge_str,
        })?;

        Ok(())
    }

    fn from_query_params<'a, Q>(_query_iter: &mut Q) -> Result<Self, ()>
    where
        Q: QueryIter<'a>,
    {
        todo!()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PairPhase3Response {
    pub paired: bool,
    pub server_pairing_secret: Vec<u8>,
}

impl TextResponse for PairPhase3Response {
    fn serialize_into(&self, _body_writer: &mut impl fmt::Write) -> fmt::Result {
        todo!()
    }
}

impl FromStr for PairPhase3Response {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let doc = Document::parse(s)?;
        let root = parse_xml_root_node(&doc)?;

        let paired = parse_xml_child_paired(root)?;

        let pairing_secret_str = parse_xml_child_text(root, "pairingsecret")?;
        let pairing_secret = hex::decode(pairing_secret_str)?;

        Ok(PairPhase3Response {
            paired,
            server_pairing_secret: pairing_secret,
        })
    }
}
