use std::str::FromStr;

use reqwest::{
    Certificate,
    blocking::{self, ClientBuilder},
};
use tracing::{debug, instrument};

use crate::http::{
    Endpoint, ParseError, TextResponse,
    client::{
        DEFAULT_LONG_TIMEOUT, DEFAULT_TIMEOUT,
        blocking_client::RequestClient,
        reqwest::{ReqwestError, build_url},
    },
};

pub type ReqwestClient = blocking::Client;

fn default_builder() -> ClientBuilder {
    ClientBuilder::new()
        .timeout(DEFAULT_LONG_TIMEOUT)
        // Use rustls because other backends could have varying support for custom certs
        // e.g. schannel (windows)
        .use_rustls_tls()
        // https://github.com/seanmonstar/reqwest/issues/2021
        .pool_max_idle_per_host(0)
        // Sunshine only likes http 1.0
        .http1_only()
}
fn timeout_builder() -> ClientBuilder {
    default_builder().timeout(DEFAULT_TIMEOUT)
}

impl RequestClient for ReqwestClient {
    type Error = ReqwestError;

    fn with_defaults() -> Result<Self, Self::Error> {
        Ok(default_builder().build()?)
    }

    fn with_defaults_long_timeout() -> Result<Self, Self::Error> {
        Ok(timeout_builder().build()?)
    }

    fn with_certificates(
        client_private_key: &pem::Pem,
        client_certificate: &pem::Pem,
        server_certificate: &pem::Pem,
    ) -> Result<Self, Self::Error> {
        let server_certificate = Certificate::from_pem(server_certificate.to_string().as_bytes())?;

        todo!()
    }

    #[instrument(skip(self, request), fields(path = E::path()), err)]
    fn send_http<E>(
        &self,
        client_info: crate::http::ClientInfo<'_>,
        hostport: &str,
        request: &E::Request,
    ) -> Result<E::Response, Self::Error>
    where
        E: Endpoint,
        E::Response: TextResponse<Err = ParseError>,
    {
        let url = build_url::<E>(false, client_info, hostport, request)?;

        debug!(url = %url, "sending request");

        let response_text = self.get(url).send()?.text()?;

        debug!(response = ?response_text, "received response");

        let response = <E::Response as FromStr>::from_str(&response_text)?;

        Ok(response)
    }

    #[instrument(skip(self, request), fields(path = E::path()), err)]
    fn send_https<E>(
        &self,
        client_info: crate::http::ClientInfo<'_>,
        hostport: &str,
        request: &E::Request,
    ) -> Result<E::Response, Self::Error>
    where
        E: Endpoint,
        E::Response: TextResponse<Err = ParseError>,
    {
        let url = build_url::<E>(true, client_info, hostport, request)?;

        debug!(url = %url, "sending request");

        let response_text = self.get(url).send()?.text()?;

        debug!(response = ?response_text, "received response");

        let response = <E::Response as FromStr>::from_str(&response_text)?;

        Ok(response)
    }

    #[instrument(skip(self, request), fields(path = E::path()), err)]
    fn send_https_with_bytes<E>(
        &self,
        client_info: crate::http::ClientInfo<'_>,
        hostport: &str,
        request: &E::Request,
    ) -> Result<E::Response, Self::Error>
    where
        E: Endpoint<Response = Vec<u8>>,
    {
        let url = build_url::<E>(false, client_info, hostport, request)?;

        debug!(url = %url, "sending request");

        let response = self.get(url).send()?.bytes()?;

        debug!(response = ?response, "received response");

        Ok(response.into())
    }
}
