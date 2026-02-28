use std::{str::FromStr, sync::Arc};

use bytes::Bytes;
use http_body_util::{BodyExt, Empty};
use hyper::{Request, Response, Uri, body::Incoming, client::conn::http1};
use hyper_rustls::HttpsConnector;
use hyper_util::{
    client::legacy::{Client, connect::HttpConnector},
    rt::{TokioExecutor, TokioIo},
};
use rustls::{
    ClientConfig, DigitallySignedStruct, RootCertStore, SignatureScheme,
    client::{
        Resumption, WebPkiServerVerifier,
        danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
    },
    pki_types::{
        CertificateDer, PrivateKeyDer, ServerName, UnixTime,
        pem::{PemObject, SectionKind},
    },
    server::VerifierBuilderError,
};
use thiserror::Error;
use tokio::{
    io::{self, AsyncWriteExt},
    net::TcpStream,
    task::JoinError,
};
use tracing::{debug, error, instrument};

use crate::http::{
    ClientInfo, Endpoint, ParseError, TextResponse,
    client::{
        DEFAULT_LONG_TIMEOUT, DEFAULT_TIMEOUT, RequestError, async_client::RequestClient, build_url,
    },
};

#[derive(Debug, Error)]
pub enum HyperError {
    #[error("hyper client: {0}")]
    HyperClient(#[from] hyper_util::client::legacy::Error),
    #[error("hyper: {0}")]
    Hyper(#[from] hyper::Error),
    #[error("rustls: {0}")]
    Rustls(#[from] rustls::Error),
    #[error("webpki build server certificate verifier: {0}")]
    WebPkiBuildVerifier(#[from] VerifierBuilderError),
    #[error("awc client tried to use an invalid private key")]
    InvalidPrivateKey,
    #[error("join: {0}")]
    Join(#[from] JoinError),
    #[error("{0}")]
    UrlParse(#[from] url::ParseError),
    #[error("response: {0}")]
    Parse(#[from] ParseError),
}

impl RequestError for HyperError {
    fn is_connect(&self) -> bool {
        todo!()
    }
    fn is_encryption(&self) -> bool {
        todo!()
    }
}

impl TryInto<ParseError> for HyperError {
    type Error = Self;

    fn try_into(self) -> Result<ParseError, Self::Error> {
        match self {
            Self::Parse(parse) => Ok(parse),
            _ => Err(self),
        }
    }
}

#[derive(Debug)]
struct NoHostnameVerifier<Base> {
    base: Base,
}

impl<Base> ServerCertVerifier for NoHostnameVerifier<Base>
where
    Base: ServerCertVerifier,
{
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        self.base.verify_tls12_signature(message, cert, dss)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        self.base.verify_tls13_signature(message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.base.supported_verify_schemes()
    }
}

async fn response_to_bytes(mut response: Response<Incoming>) -> Result<Vec<u8>, HyperError> {
    let mut bytes = Vec::new();
    // Stream the body, writing each chunk to our response buffer
    while let Some(next) = response.frame().await {
        let frame = next?;
        if let Some(chunk) = frame.data_ref() {
            bytes.extend_from_slice(chunk);
        }
    }

    Ok(bytes)
}

#[derive(Debug, Clone)]
pub struct HyperClient {
    client: Client<HttpsConnector<HttpConnector>, Empty<Bytes>>,
}

impl RequestClient for HyperClient {
    type Error = HyperError;

    fn with_defaults_long_timeout() -> Result<Self, Self::Error> {
        let config = ClientConfig::builder()
            .with_root_certificates(RootCertStore::empty())
            .with_no_client_auth();

        // Build the hyper rustls connector
        let https = hyper_rustls::HttpsConnectorBuilder::new()
            .with_tls_config(config)
            .https_or_http()
            .enable_http1()
            .build();

        // Build Client
        let client = Client::builder(TokioExecutor::new())
            .pool_max_idle_per_host(0)
            .build(https);

        Ok(Self { client })
    }

    fn with_defaults() -> Result<Self, Self::Error> {
        let config = ClientConfig::builder()
            .with_root_certificates(RootCertStore::empty())
            .with_no_client_auth();

        // Build the hyper rustls connector
        let https = hyper_rustls::HttpsConnectorBuilder::new()
            .with_tls_config(config)
            .https_or_http()
            .enable_http1()
            .build();

        // Build Client
        let client = Client::builder(TokioExecutor::new())
            .pool_max_idle_per_host(0)
            .build(https);

        Ok(Self { client })
    }

    #[cfg_attr(feature = "__tracing_sensitive", instrument(err))]
    fn with_certificates(
        client_private_key: &pem::Pem,
        client_certificate: &pem::Pem,
        server_certificate: &pem::Pem,
    ) -> Result<Self, Self::Error> {
        // Client
        if !client_private_key.tag().eq_ignore_ascii_case("PRIVATE KEY") {
            return Err(HyperError::InvalidPrivateKey);
        }
        let private_key = PrivateKeyDer::from_pem(
            SectionKind::PrivateKey,
            client_private_key.contents().to_vec(),
        )
        .ok_or(HyperError::InvalidPrivateKey)?
        .clone_key();

        let certificate = CertificateDer::from_slice(client_certificate.contents()).into_owned();

        // Server
        let mut root_certificates = RootCertStore::empty();
        root_certificates.add(CertificateDer::from_slice(server_certificate.contents()))?;
        let root_certificates = Arc::new(root_certificates);

        // Create Config
        let mut config = ClientConfig::builder()
            .with_root_certificates(root_certificates.clone())
            .with_client_auth_cert(vec![certificate], private_key)?;

        // Disable resumption, Sunshine cannot handle them
        config.resumption = Resumption::disabled();

        // Create custom server verifier that doesn't care about host names
        let verifier = NoHostnameVerifier {
            // The builder doesn't store the Arc reference anywhere so we can move the value out of the Arc
            #[allow(clippy::unwrap_used)]
            base: Arc::try_unwrap(WebPkiServerVerifier::builder(root_certificates).build()?)
                .unwrap(),
        };
        config
            .dangerous()
            .set_certificate_verifier(Arc::new(verifier));

        // Build the hyper rustls connector
        let https = hyper_rustls::HttpsConnectorBuilder::new()
            .with_tls_config(config)
            .https_or_http()
            .enable_http1()
            .build();

        // Build Client
        let client = Client::builder(TokioExecutor::new())
            .pool_max_idle_per_host(0)
            .build(https);

        Ok(Self { client })
    }

    #[instrument(skip(self, request), fields(path = E::path()), err)]
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

        debug!(url = %url, "sending request");

        let response = self.client.get(url.as_str().parse().unwrap()).await?;
        let response_bytes = response_to_bytes(response).await?;
        let response_text = String::from_utf8_lossy(&response_bytes);

        debug!(response = ?response_text, "received response");

        Ok(E::Response::from_str(&response_text)?)
    }

    #[instrument(skip(self, request), fields(path = E::path()), err)]
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

        debug!(url = %url, "sending request");

        let response = self.client.get(url.as_str().parse().unwrap()).await?;
        let response_bytes = response_to_bytes(response).await?;
        let response_text = String::from_utf8_lossy(&response_bytes);

        debug!(response = ?response_text, "received response");

        Ok(E::Response::from_str(&response_text)?)
    }

    #[instrument(skip(self, request), fields(path = E::path()), err)]
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
        let url = build_url::<E>(true, client_info, hostport, request)?;

        debug!(url = %url, "sending request");

        todo!();

        // debug!("received response");

        // Ok(response.into())
    }
}
