use std::{fmt, str::FromStr};

use roxmltree::Document;

use crate::http::{
    ParseError, QueryBuilder, QueryBuilderError, QueryIter, QueryParam, Request, TextResponse,
    helper::parse_xml_root_node, pair::parse_xml_child_paired,
};

#[derive(Debug, Clone, PartialEq)]
pub struct PairPhase4Request {
    pub device_name: String,
    pub client_pairing_secret: Vec<u8>,
}

impl Request for PairPhase4Request {
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

        let client_pairing_secret_str = hex::encode_upper(&self.client_pairing_secret);
        query_builder.append(QueryParam {
            key: "clientpairingsecret",
            value: &client_pairing_secret_str,
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
pub struct PairPhase4Response {
    pub paired: bool,
}

impl TextResponse for PairPhase4Response {
    fn serialize_into(&self, body_writer: &mut impl fmt::Write) -> fmt::Result {
        todo!()
    }
}

impl FromStr for PairPhase4Response {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let doc = Document::parse(s)?;
        let root = parse_xml_root_node(&doc)?;

        let paired = parse_xml_child_paired(root)?;

        Ok(Self { paired })
    }
}
