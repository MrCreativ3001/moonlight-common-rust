use std::{
    fmt::{self, Debug},
    net::AddrParseError,
    num::ParseIntError,
    str::FromStr,
    string::FromUtf8Error,
};

use pem::Pem;
use roxmltree::Error;
use thiserror::Error;
use uuid::{Uuid, adapter::Hyphenated};

use crate::{ParseServerStateError, ParseServerVersionError, mac::ParseMacError};

#[derive(Debug, Error, PartialEq)]
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
    #[error("mac: {0}")]
    ParseMacError(#[from] ParseMacError),
    #[error("int: {0}")]
    ParseIntError(#[from] ParseIntError),
    #[error("uuid: {0}")]
    ParseUuidError(#[from] uuid::Error),
    #[error("hex: {0}")]
    ParseHexError(#[from] hex::FromHexError),
    #[error("addr: {0}")]
    ParseAddrError(#[from] AddrParseError),
    #[error("pem: {0}")]
    ParsePem(#[from] pem::PemError),
    #[error("utf-8: {0}")]
    Utf8Error(#[from] FromUtf8Error),
}

pub mod app_list;
pub mod box_art;
pub mod cancel;
pub mod launch;
pub mod pair;
pub mod resume;
pub mod server_info;
pub mod unpair;

pub mod client;

mod helper;

#[allow(clippy::unwrap_used)]
#[cfg(test)]
mod test;

// TODO: what is the correct way to handle errors in this system when receiving messages from a client (server impl)? e.g. failed to start stream, not authenticated for app endpoint?

pub const DEFAULT_HTTP_PORT: u16 = 47989;
pub const DEFAULT_HTTPS_PORT: u16 = 47984;

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
/// # Custom Client or Server
///
/// ## Client Usage
/// Use the [client::async_client::RequestClient] or [client::blocking_client::RequestClient] when possible to make integration into other systems easier.
///
/// ```
/// // Get client info and request
/// let client_info = ClientInfo {
///     unique_id: DEFAULT_UNIQUE_ID,
///     uuid: Uuid::new_v4(),
/// };
///
/// type Endpoint = ...;
/// let request: Endpoint::Request = ...;
///
/// // If [Endpoint::https_required] is true, only authenticated https requests are allowed
/// let mut url = Url::parse(format!("http:127.0.0.1:47989{}", Endpoint::path()));
///
/// // Append client information to url
/// client_info.append_query_parameters(&mut url).unwrap();
///
/// // Append request query parameters to url
/// request.append_query_parameters(&mut url).unwrap();
///
/// // Send a get request to the url and turn the response into a string
/// let text_response: String = my_request_client.send_get_request(url).unwrap();
///
/// // Almost all responses are of type TextResponse
/// let response: Endpoint::Response = Endpoint::Response::from_str(&text_response).unwrap();
///
/// // Some endpoints might also return a `Vec<u8>` for raw bytes (e.g. images).
/// // Those don't need to be converted and can directly be used.
///
/// ```
///
/// For a real implementation see the [client::async_client::RequestClient] implementation of [reqwest::Client]
///
/// ## Server Usage
///
/// TODO
///
pub trait Endpoint {
    type Request: Request;
    type Response;

    /// The path of this endpoint. Always begins with a `/`.
    fn path() -> &'static str;

    /// If this endpoint requires https / authentication
    ///
    /// If this returns false an authenticated response could still return a different result than an unauthenticated response.
    fn https_required() -> bool;
}

pub trait Request: Sized {
    /// Serialize the parameters in this request into the query builder.
    fn append_query_params(
        &self,
        query_builder: &mut impl QueryBuilder,
    ) -> Result<(), QueryBuilderError>;

    // TODO: maybe don't use an iterator, but some kind of map like interface?
    // TODO: error?
    /// Parse the query parameters of into this request type.
    fn from_query_params<'a, Q>(query_iter: &mut Q) -> Result<Self, ()>
    where
        Q: QueryIter<'a>;
}

pub trait TextResponse: FromStr {
    fn serialize_into(&self, body_writer: &mut impl fmt::Write) -> fmt::Result;
}

/// It's recommended to use the same (default) UID for all Moonlight clients so we can quit games started by other Moonlight clients.
pub const DEFAULT_UNIQUE_ID: &str = "0123456789ABCDEF";

/// The identifier of a client.
/// Every client request should use this, even when unauthenticated.
#[derive(Debug, Clone, Copy, PartialEq)]
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

impl<'b> Request for ClientInfo<'b> {
    fn append_query_params(
        &self,
        query_builder: &mut impl QueryBuilder,
    ) -> Result<(), QueryBuilderError> {
        query_builder.append(QueryParam {
            key: "uniqueid",
            value: self.unique_id,
        })?;

        let mut uuid_bytes = [0; Hyphenated::LENGTH];
        self.uuid.to_hyphenated_ref().encode_lower(&mut uuid_bytes);
        let uuid_str = str::from_utf8(&uuid_bytes).expect("uuid string");

        query_builder.append(QueryParam {
            key: "uuid",
            value: uuid_str,
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

// TODO: use those types instead of directly using Pem
// TODO: make a from_pem_str fn, so you don't need to include the pem lib
// TODO: maybe arc the data?
// TODO: use the der data instead of the whole cert / pk

/// This is used to identify and verify a server.
#[derive(Debug, Clone, PartialEq)]
pub struct ServerIdentifier(Pem);

impl ServerIdentifier {
    pub fn from_pem(pem: Pem) -> Self {
        // TODO: check for the correct header or tag
        Self(pem)
    }

    pub fn to_pem(&self) -> Pem {
        self.0.clone()
    }
}

/// This is used to identify and verify a client.
#[derive(Debug, Clone, PartialEq)]
pub struct ClientIdentifier(Pem);

impl ClientIdentifier {
    pub fn from_pem(pem: Pem) -> Self {
        // TODO: check for the correct header or tag
        Self(pem)
    }

    pub fn to_pem(&self) -> Pem {
        self.0.clone()
    }
}

/// The secret of the client.
/// This MUST NOT be shared and MUST be kept secret.
#[derive(Clone, PartialEq)]
#[cfg_attr(feature = "__tracing_sensitive", derive(Debug))]
pub struct ClientSecret(Pem);

impl ClientSecret {
    pub fn from_pem(pem: Pem) -> Self {
        // TODO: check for the correct header or tag
        Self(pem)
    }

    pub fn to_pem(&self) -> Pem {
        self.0.clone()
    }
}

#[cfg(not(feature = "__tracing_sensitive"))]
impl Debug for ClientSecret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[ClientSecret]")
    }
}
