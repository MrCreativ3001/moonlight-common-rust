use std::str::FromStr;

use pem::Pem;
use roxmltree::Document;

use crate::http::{
    ParseError, QueryBuilder, QueryBuilderError, QueryIter, QueryParam, Request, TextResponse,
    helper::{parse_xml_child_text, parse_xml_root_node},
    pair::{SALT_LENGTH, parse_xml_child_paired},
};

#[derive(Debug, Clone, PartialEq)]
pub struct PairPhase1Request {
    pub device_name: String,
    pub salt: [u8; SALT_LENGTH],
    pub client_certificate: Pem,
}

impl Request for PairPhase1Request {
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

        query_builder.append(QueryParam {
            key: "phrase",
            value: "getservercert",
        })?;

        let salt_str = hex::encode_upper(self.salt);
        query_builder.append(QueryParam {
            key: "salt",
            value: &salt_str,
        })?;

        let client_cert_pem_str = hex::encode_upper(&self.client_certificate.to_string());
        query_builder.append(QueryParam {
            key: "clientcert",
            value: &client_cert_pem_str,
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

pub struct PairPhase1Response {
    pub paired: bool,
    pub certificate: Option<Pem>,
}

impl TextResponse for PairPhase1Response {
    fn serialize_into(&self, body_writer: &mut impl std::fmt::Write) -> std::fmt::Result {
        todo!()
    }
}

impl FromStr for PairPhase1Response {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let doc = Document::parse(s)?;
        let root = parse_xml_root_node(&doc)?;

        let paired = parse_xml_child_paired(root)?;

        let certificate = match parse_xml_child_text(root, "plaincert") {
            Ok(value) => {
                let value = hex::decode(value)?;
                let str = String::from_utf8(value)?;

                let pem = Pem::from_str(&str)?;
                Some(pem)
            }
            Err(ParseError::DetailNotFound("plaincert")) => None,
            Err(err) => return Err(err),
        };

        Ok(Self {
            paired,
            certificate,
        })
    }
}
