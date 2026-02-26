use std::{str::FromStr, time::Duration};

use pem::Pem;
use reqwest::{Certificate, Client, ClientBuilder, Identity};
use thiserror::Error;
use url::Url;

use crate::http::{
    ClientInfo, Endpoint, ParseError, Request, TextResponse,
    client::{DEFAULT_LONG_TIMEOUT, DEFAULT_TIMEOUT, RequestError, async_client::RequestClient},
};

pub type ReqwestClient = reqwest::Client;

#[derive(Debug, Error)]
pub enum ReqwestError {
    #[error("{0}")]
    Reqwest(#[from] reqwest::Error),
    #[error("{0}")]
    UrlParse(#[from] url::ParseError),
    #[error("response: {0}")]
    Parse(#[from] ParseError),
}
pub type ReqwestApiError = ReqwestError;

impl RequestError for ReqwestError {
    fn is_connect(&self) -> bool {
        matches!(self, ReqwestError::Reqwest(err) if err.is_connect() || err.is_timeout())
    }
    fn is_encryption(&self) -> bool {
        match self {
            ReqwestError::Reqwest(err) => err.is_decode(),
            _ => false,
        }
    }
}

impl TryInto<ParseError> for ReqwestError {
    type Error = Self;

    fn try_into(self) -> Result<ParseError, Self::Error> {
        match self {
            Self::Parse(parse) => Ok(parse),
            _ => Err(self),
        }
    }
}

fn default_builder() -> ClientBuilder {
    ClientBuilder::new()
        // Use rustls because other backends could have varying support for custom certs
        // e.g. schannel (windows)
        .tls_backend_rustls()
        .timeout(DEFAULT_LONG_TIMEOUT)
        // https://github.com/seanmonstar/reqwest/issues/2021
        .pool_max_idle_per_host(0)
        .pool_idle_timeout(Some(Duration::ZERO))
}
fn timeout_builder() -> ClientBuilder {
    default_builder().timeout(DEFAULT_TIMEOUT)
}

fn build_url<E>(
    use_https: bool,
    client_info: ClientInfo<'_>,
    hostport: &str,
    request: &E::Request,
) -> Result<Url, ReqwestError>
where
    E: Endpoint,
{
    let protocol = if use_https { "https" } else { "http" };

    let authority = format!("{protocol}://{hostport}{}", E::path());
    let mut url = Url::parse(&authority)?;

    client_info.append_query_params(&mut url);

    request.append_query_params(&mut url);

    Ok(url)
}

impl RequestClient for Client {
    type Error = ReqwestError;

    fn with_defaults_long_timeout() -> Result<Self, Self::Error> {
        Ok(default_builder().build()?)
    }
    fn with_defaults() -> Result<Self, Self::Error> {
        Ok(timeout_builder().build()?)
    }

    fn with_certificates(
        client_private_key: &Pem,
        client_certificate: &Pem,
        server_certificate: &Pem,
    ) -> Result<Self, Self::Error> {
        let server_cert = Certificate::from_pem(server_certificate.to_string().as_bytes())?;

        let mut client_pem = String::new();
        client_pem.push_str(&client_private_key.to_string());
        client_pem.push('\n');
        client_pem.push_str(&client_certificate.to_string());

        let identity = Identity::from_pem(client_pem.as_bytes())?;

        Ok(timeout_builder()
            .tls_certs_only([server_cert])
            .identity(identity)
            .build()?)
    }

    async fn send_http<E>(
        &self,
        client_info: ClientInfo<'_>,
        hostport: &str,
        request: &E::Request,
    ) -> Result<E::Response, Self::Error>
    where
        E: Endpoint,
        E::Request: Sync,
        E::Response: TextResponse<Err = ParseError>,
    {
        let url = build_url::<E>(false, client_info, hostport, request)?;

        let response_text = self.get(url).send().await?.text().await?;

        let response = <E::Response as FromStr>::from_str(&response_text)?;

        Ok(response)
    }

    async fn send_https<E>(
        &self,
        client_info: ClientInfo<'_>,
        hostport: &str,
        request: &E::Request,
    ) -> Result<E::Response, Self::Error>
    where
        E: Endpoint,
        E::Request: Sync,
        E::Response: TextResponse<Err = ParseError>,
    {
        let url = build_url::<E>(true, client_info, hostport, request)?;

        let response_text = self.get(url).send().await?.text().await?;

        let response = <E::Response as FromStr>::from_str(&response_text)?;

        Ok(response)
    }

    async fn send_https_with_bytes<E>(
        &self,
        client_info: ClientInfo<'_>,
        hostport: &str,
        request: &E::Request,
    ) -> Result<E::Response, Self::Error>
    where
        E: Endpoint<Response = Vec<u8>>,
        E::Request: Sync,
    {
        let url = build_url::<E>(false, client_info, hostport, request)?;

        let response = self.get(url).send().await?.bytes().await?;

        Ok(response.into())
    }
}
