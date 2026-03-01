use std::fmt;
use std::str::FromStr;

use roxmltree::Document;

use crate::http::{
    Endpoint, ParseError, QueryBuilder, QueryBuilderError, QueryIter, Request, TextResponse,
    helper::parse_xml_child_text,
};

pub struct CancelEndpoint;

impl Endpoint for CancelEndpoint {
    type Request = CancelRequest;
    type Response = CancelResponse;

    fn path() -> &'static str {
        "/cancel"
    }

    fn https_required() -> bool {
        true
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CancelRequest {}

impl Request for CancelRequest {
    fn append_query_params(
        &self,
        _query_builder: &mut impl QueryBuilder,
    ) -> Result<(), QueryBuilderError> {
        Ok(())
    }

    fn from_query_params<'a, Q>(_query_iter: &mut Q) -> Result<Self, ()>
    where
        Q: QueryIter<'a>,
    {
        Ok(Self {})
    }
}

#[derive(Debug, PartialEq)]
pub struct CancelResponse {
    pub cancelled: bool,
}

impl TextResponse for CancelResponse {
    fn serialize_into(&self, body_writer: &mut impl fmt::Write) -> fmt::Result {
        write!(
            body_writer,
            r#"<?xml version="1.0" encoding="utf-8"?><root status_code="200"><cancel>{}</cancel></root>"#,
            if self.cancelled { 1 } else { 0 },
        )
    }
}

impl FromStr for CancelResponse {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let doc = Document::parse(s)?;
        let root = doc
            .root()
            .children()
            .find(|node| node.tag_name().name() == "root")
            .ok_or(ParseError::XmlRootNotFound)?;

        let cancel = parse_xml_child_text(root, "cancel")?.trim();

        Ok(Self {
            cancelled: cancel != "0",
        })
    }
}
