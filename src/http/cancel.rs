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
    pub cancel: bool,
}

impl TextResponse for CancelResponse {
    fn serialize_into(&self, body_writer: &mut impl std::fmt::Write) -> std::fmt::Result {
        todo!()
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

        // TODO: what is this? https://github.com/moonlight-stream/moonlight-android/blob/f10085f552b367cf7203007693d91c322a0a2936/app/src/main/java/com/limelight/nvstream/http/NvHTTP.java#L803-L818
        Ok(Self {
            cancel: cancel != "0",
        })
    }
}
