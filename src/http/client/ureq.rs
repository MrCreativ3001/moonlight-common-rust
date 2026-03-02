use std::{str::FromStr, sync::Arc};

use pem::Pem;
use thiserror::Error;
use tracing::{debug, instrument};
use ureq::{
    Agent,
    config::Config,
    tls::{Certificate, ClientCert, PrivateKey, RootCerts, TlsConfig, TlsProvider},
};

use crate::http::{
    ClientInfo, Endpoint, ParseError, TextResponse,
    client::{
        DEFAULT_LONG_TIMEOUT, DEFAULT_TIMEOUT, RequestError, blocking_client::RequestClient,
        hyperlike::build_url,
    },
};

pub type UreqClient = Config;

#[derive(Debug, Error)]
pub enum UreqError {
    #[error("ureq: {0}")]
    Ureq(#[from] ureq::Error),
    #[error("parse: {0}")]
    Parse(#[from] ParseError),
    #[error("http: {0}")]
    Http(#[from] http::Error),
}

impl RequestError for UreqError {
    fn is_connect(&self) -> bool {
        matches!(
            self,
            Self::Ureq(ureq::Error::HostNotFound)
                | Self::Ureq(ureq::Error::ConnectionFailed)
                | Self::Ureq(ureq::Error::Io(_))
        )
    }
    fn is_encryption(&self) -> bool {
        matches!(self, Self::Ureq(ureq::Error::Tls(_)))
    }
}

impl TryInto<ParseError> for UreqError {
    type Error = Self;

    fn try_into(self) -> Result<ParseError, Self::Error> {
        match self {
            Self::Parse(err) => Ok(err),
            _ => Err(self),
        }
    }
}

impl RequestClient for UreqClient {
    type Error = UreqError;

    fn with_defaults() -> Result<Self, Self::Error> {
        let config = Agent::config_builder()
            .timeout_global(Some(DEFAULT_TIMEOUT))
            .build();

        Ok(config)
    }
    fn with_defaults_long_timeout() -> Result<Self, Self::Error> {
        let config = Agent::config_builder()
            .timeout_global(Some(DEFAULT_LONG_TIMEOUT))
            .build();

        Ok(config)
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
    ) -> Result<Self, Self::Error> {
        let client_certificate = Certificate::from_der(client_certificate.contents()).to_owned();
        let client_private_key = PrivateKey::from_pem(client_private_key.to_string().as_bytes())?;

        let server_certificate = Certificate::from_der(server_certificate.contents()).to_owned();

        let config = Agent::config_builder()
            .timeout_global(Some(DEFAULT_TIMEOUT))
            .tls_config(
                TlsConfig::builder()
                    .provider(TlsProvider::Rustls)
                    .client_cert(Some(ClientCert::new_with_certs(
                        &[client_certificate],
                        client_private_key,
                    )))
                    .root_certs(RootCerts::Specific(Arc::new(vec![server_certificate])))
                    // TODO: THIS MUST BE CHANGED
                    .disable_verification(true)
                    .build(),
            )
            .build();

        Ok(config)
    }

    #[instrument(target = "moonlight::client::ureq", skip(self, request), fields(path = E::path()), err)]
    fn send_http<E>(
        &self,
        client_info: ClientInfo<'_>,
        hostport: &str,
        request: &E::Request,
    ) -> Result<E::Response, Self::Error>
    where
        E: Endpoint,
        E::Response: TextResponse<Err = ParseError>,
    {
        let url = build_url::<E, UreqError>(false, client_info, hostport, request)?;

        debug!(url = %url,"sending request");

        let request_builder = Agent::new_with_config(self.clone()).get(url);
        let response = request_builder.call()?;
        let response_text = response.into_body().read_to_string()?;

        debug!(response = ?response_text, "received response");

        Ok(E::Response::from_str(&response_text)?)
    }

    #[instrument(target = "moonlight::client::ureq", skip(self, request), fields(path = E::path()), err)]
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
        let url = build_url::<E, UreqError>(true, client_info, hostport, request)?;

        debug!(url = %url,"sending request");

        let request_builder = Agent::new_with_config(self.clone()).get(url);
        let response = request_builder.call()?;
        let response_text = response.into_body().read_to_string()?;

        debug!(response = ?response_text, "received response");

        Ok(E::Response::from_str(&response_text)?)
    }

    #[instrument(target = "moonlight::client::ureq", skip(self, request), fields(path = E::path()), err)]
    fn send_https_with_bytes<E>(
        &self,
        client_info: crate::http::ClientInfo<'_>,
        hostport: &str,
        request: &E::Request,
    ) -> Result<E::Response, Self::Error>
    where
        E: Endpoint<Response = Vec<u8>>,
    {
        let url = build_url::<E, UreqError>(true, client_info, hostport, request)?;

        debug!(url = %url,"sending request");

        let request_builder = Agent::new_with_config(self.clone()).get(url);
        let response = request_builder.call()?;
        let response_bytes = response.into_body().read_to_vec()?;

        Ok(response_bytes)
    }
}
