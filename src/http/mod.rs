use std::{
    fmt::{self, Write as _},
    net::AddrParseError,
    num::ParseIntError,
    str::FromStr,
    string::FromUtf8Error,
};

use roxmltree::Error;
use thiserror::Error;
use uuid::{Uuid, adapter::Hyphenated};

use crate::{
    PairStatus, ParseServerStateError, ParseServerVersionError, mac::ParseMacAddressError,
};

#[derive(Debug, Error)]
pub enum ParseError {
    #[error("the response is invalid xml")]
    ParseXmlError(#[from] Error),
    #[error("the returned xml doc has a non 200 status code")]
    InvalidXmlStatusCode { message: Option<String> },
    #[error("the returned xml doc doesn't have the root node")]
    XmlRootNotFound,
    #[error("the text contents of an xml node aren't present: {0}")]
    XmlTextNotFound(&'static str),
    #[error("detail was not found: {0}")]
    DetailNotFound(&'static str),
    #[error("{0}")]
    ParseServerStateError(#[from] ParseServerStateError),
    #[error("{0}")]
    ParseServerVersionError(#[from] ParseServerVersionError),
    #[error("parsing server codec mode support")]
    ParseServerCodecModeSupport,
    #[error("failed to parse the mac address")]
    ParseMacError(#[from] ParseMacAddressError),
    #[error("{0}")]
    ParseIntError(#[from] ParseIntError),
    #[error("{0}")]
    ParseUuidError(#[from] uuid::Error),
    #[error("{0}")]
    ParseHexError(#[from] hex::FromHexError),
    #[error("{0}")]
    ParseAddrError(#[from] AddrParseError),
    #[error("{0}")]
    Utf8Error(#[from] FromUtf8Error),
}

pub mod app_list;
pub mod box_art;
pub mod cancel;
pub mod host_info;
pub mod launch;
pub mod resume;
//pub mod pair;

mod helper;

#[cfg(test)]
mod test;

// TODO: don't make this async depedendant but make this work in async and sync!

#[derive(Debug)]
pub struct QueryParam<'a> {
    pub key: &'a str,
    pub value: &'a str,
}

#[derive(Debug, Error)]
pub enum QueryBuilderError {
    #[error("the query builder buffer is full")]
    BufferFull,
}

pub trait QueryBuilder {
    fn append(&mut self, param: QueryParam) -> Result<(), QueryBuilderError>;
}

pub trait QueryIter<'a>: Iterator<Item = &'a QueryParam<'a>> {}
impl<'a, T> QueryIter<'a> for T where T: Iterator<Item = &'a QueryParam<'a>> {}

/// This represents an endpoint on the http or https server that a client can query for information or initiate a stream with.
///
/// ## Client Usage
///
/// ```
/// type Endpoint = ...;
///
/// Use ClientInfo
/// TODO
/// ```
///
/// ## Server Usage
///
/// TODO
///
pub trait Endpoint {
    type Request: Request;
    type Response;

    /// The path of this endpoint
    fn path() -> &'static str;

    /// If this endpoint requires https / authentication
    ///
    /// If this returns false an authenticated response could still return a different result than an unauthenticated response.
    fn https_required() -> bool;
}

pub trait Request: Sized {
    fn append_query_params(
        &self,
        query_builder: &mut impl QueryBuilder,
    ) -> Result<(), QueryBuilderError>;

    fn from_query_params<'a, Q>(query_iter: &mut Q) -> Result<Self, ()>
    where
        Q: QueryIter<'a>;
}

pub trait TextResponse: FromStr {
    fn serialize_into(&self, body_writer: &mut impl fmt::Write) -> fmt::Result;
}

pub const DEFAULT_UNIQUE_ID: &str = "0123456789ABCDEF";

/// The identifier of a client.
/// Every client request should use this, even when unauthenticated.
#[derive(Debug, Clone, Copy)]
pub struct ClientInfo<'a> {
    /// It's recommended to use the same (default) UID for all Moonlight clients so we can quit games started by other Moonlight clients.
    pub unique_id: &'a str,
    pub uuid: Uuid,
}

impl Default for ClientInfo<'static> {
    fn default() -> Self {
        Self {
            unique_id: DEFAULT_UNIQUE_ID,
            uuid: Uuid::new_v4(),
        }
    }
}

impl<'a> ClientInfo<'a> {
    // Requires 2 query params
    fn push_query_params(
        &self,
        uuid_bytes: &'a mut [u8; Hyphenated::LENGTH],
        query_params: &mut impl QueryBuilder,
    ) {
        query_params.append(QueryParam {
            key: "uniqueid",
            value: self.unique_id,
        });

        self.uuid.to_hyphenated_ref().encode_lower(uuid_bytes);
        let uuid_str = str::from_utf8(uuid_bytes).expect("uuid string");

        query_params.append(QueryParam {
            key: "uuid",
            value: uuid_str,
        });
    }
}
