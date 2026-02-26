use std::{fmt, str::FromStr};

use crate::http::{Endpoint, ParseError, QueryBuilderError, QueryIter, Request, TextResponse};

/// TODO
///
/// References:
/// - Moonlight-Embedded: https://github.com/moonlight-stream/moonlight-embedded/blob/775444287305849ebdf4736c75298ad0713e2d5d/libgamestream/client.c#L408-L424
// TODO: how does this endpoint work?
pub struct UnpairEndpoint;

impl Endpoint for UnpairEndpoint {
    type Request = UnpairRequest;
    type Response = UnpairResponse;

    fn https_required() -> bool {
        false
    }

    fn path() -> &'static str {
        "/unpair"
    }
}

#[derive(Debug, PartialEq)]
pub struct UnpairRequest {}

impl Request for UnpairRequest {
    fn append_query_params(
        &self,
        _query_builder: &mut impl super::QueryBuilder,
    ) -> Result<(), QueryBuilderError> {
        Ok(())
    }

    fn from_query_params<'a, Q>(query_iter: &mut Q) -> Result<Self, ()>
    where
        Q: QueryIter<'a>,
    {
        Ok(Self {})
    }
}

pub struct UnpairResponse {}

impl TextResponse for UnpairResponse {
    fn serialize_into(&self, _body_writer: &mut impl fmt::Write) -> fmt::Result {
        Ok(())
    }
}

impl FromStr for UnpairResponse {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self {})
    }
}
