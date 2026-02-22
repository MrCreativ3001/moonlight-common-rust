use bytes::Bytes;
use pem::Pem;

use crate::http::{ClientInfo, Endpoint, ParseError, TextResponse, client::RequestError};

///
/// A blocking request client that can make requests to an [Endpoint].
///
pub trait RequestClient: Sized {
    type Error: RequestError;

    type Text: AsRef<str>;
    type Bytes: AsRef<[u8]>;

    fn with_defaults() -> Result<Self, Self::Error>;
    fn with_defaults_long_timeout() -> Result<Self, Self::Error>;

    fn with_certificates(
        client_private_key: &Pem,
        client_certificate: &Pem,
        server_certificate: &Pem,
    ) -> Result<Self, Self::Error>;

    fn send_http<E>(
        &self,
        client_info: ClientInfo<'_>,
        hostport: &str,
        request: &E::Request,
    ) -> Result<E::Response, Self::Error>
    where
        E: Endpoint,
        E::Response: TextResponse<Err = ParseError>;

    fn send_https<E>(
        &self,
        client_info: ClientInfo<'_>,
        hostport: &str,
        request: &E::Request,
    ) -> Result<E::Response, Self::Error>
    where
        E: Endpoint,
        E::Response: TextResponse<Err = ParseError>;

    fn send_https_with_bytes<E>(
        &self,
        client_info: ClientInfo<'_>,
        hostport: &str,
        request: &E::Request,
    ) -> Result<E::Response, Self::Error>
    where
        E: Endpoint<Response = Bytes>;
}
