use std::{str::FromStr, sync::Arc};

use pem::Pem;
use tracing::{debug, instrument, trace};

use crate::{
    error::Error,
    http::{
        ClientInfo, Endpoint, TextResponse,
        client::{DEFAULT_LONG_TIMEOUT, DEFAULT_TIMEOUT, blocking_client::RequestClient},
    },
};

#[derive(Debug, Clone)]
pub struct StdClient {}

impl RequestClient for StdClient {
    fn with_defaults() -> Result<Self, Error> {
        todo!()
    }
    fn with_defaults_long_timeout() -> Result<Self, Error> {
        todo!()
    }

    #[cfg_attr(
        not(feature = "__tracing_sensitive"),
        instrument(target = "moonlight::client::ureq", skip_all, err)
    )]
    #[cfg_attr(
        feature = "__tracing_sensitive",
        instrument(target = "moonlight::client::ureq", err)
    )]
    fn with_certificates(
        client_private_key: &Pem,
        client_certificate: &Pem,
        server_certificate: &Pem,
    ) -> Result<Self, Error> {
        todo!()
    }

    #[instrument(target = "moonlight::client::ureq", skip(self, request), fields(path = E::path()), err)]
    fn send_http<E>(
        &self,
        client_info: ClientInfo,
        hostport: &str,
        request: &E::Request,
    ) -> Result<E::Response, Error>
    where
        E: Endpoint,
        E::Response: TextResponse,
    {
        todo!()
    }

    #[instrument(target = "moonlight::client::ureq", skip(self, request), fields(path = E::path()), err)]
    fn send_https<E>(
        &self,
        client_info: ClientInfo,
        hostport: &str,
        request: &E::Request,
    ) -> Result<E::Response, Error>
    where
        E: Endpoint,
        E::Response: TextResponse,
    {
        todo!()
    }

    #[instrument(target = "moonlight::client::ureq", skip(self, request), fields(path = E::path()), err)]
    fn send_https_with_bytes<E>(
        &self,
        client_info: ClientInfo,
        hostport: &str,
        request: &E::Request,
    ) -> Result<E::Response, Error>
    where
        E: Endpoint<Response = Vec<u8>>,
    {
        todo!()
    }
}
