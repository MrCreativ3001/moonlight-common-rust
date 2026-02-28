use std::{str::FromStr, sync::Arc};

use awc::{ClientBuilder, Connector};
use rustls::{
    ClientConfig, DigitallySignedStruct, RootCertStore, SignatureScheme,
    client::{
        WebPkiServerVerifier,
        danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
    },
    pki_types::{
        CertificateDer, PrivateKeyDer, ServerName, UnixTime,
        pem::{PemObject, SectionKind},
    },
    server::VerifierBuilderError,
};
use thiserror::Error;
use tokio::task::{JoinError, spawn_local};
use tracing::{debug, instrument};

use crate::http::{
    ClientInfo, Endpoint, ParseError, TextResponse,
    client::{
        DEFAULT_LONG_TIMEOUT, DEFAULT_TIMEOUT, RequestError, async_client::RequestClient, build_url,
    },
};

pub type AwcClient = awc::Client;

#[derive(Debug, Error)]
pub enum AwcError {
    #[error("awc send: {0}")]
    AwcSend(#[from] awc::error::SendRequestError),
    #[error("awc payload: {0}")]
    AwcPayload(#[from] awc::error::PayloadError),
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

impl RequestError for AwcError {
    fn is_connect(&self) -> bool {
        todo!()
    }
    fn is_encryption(&self) -> bool {
        todo!()
    }
}

impl TryInto<ParseError> for AwcError {
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

impl RequestClient for AwcClient {
    type Error = AwcError;

    fn with_defaults_long_timeout() -> Result<Self, Self::Error> {
        let client = ClientBuilder::new().timeout(DEFAULT_LONG_TIMEOUT).finish();

        Ok(client)
    }

    fn with_defaults() -> Result<Self, Self::Error> {
        let client = ClientBuilder::new().timeout(DEFAULT_TIMEOUT).finish();

        Ok(client)
    }

    #[cfg_attr(feature = "__tracing_sensitive", instrument(err))]
    fn with_certificates(
        client_private_key: &pem::Pem,
        client_certificate: &pem::Pem,
        server_certificate: &pem::Pem,
    ) -> Result<Self, Self::Error> {
        // Client
        if !client_private_key.tag().eq_ignore_ascii_case("PRIVATE KEY") {
            return Err(AwcError::InvalidPrivateKey);
        }
        let private_key = PrivateKeyDer::from_pem(
            SectionKind::PrivateKey,
            client_private_key.contents().to_vec(),
        )
        .ok_or(AwcError::InvalidPrivateKey)?
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

        config.alpn_protocols = vec![b"http/1.1".to_vec()];

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

        // Build Client
        let connector = Connector::new().rustls_0_23(Arc::new(config));

        let client = ClientBuilder::new()
            .timeout(DEFAULT_TIMEOUT)
            .connector(connector)
            .finish();

        Ok(client)
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

        let client = self.clone();

        let join: Result<_, Self::Error> = spawn_local(async move {
            let bytes = client.get(url.as_str()).send().await?.body().await?;

            Ok(bytes)
        })
        .await?;
        let response = join?;

        // TODO: convert this to from_utf8_lossy_owned
        let response_text = String::from_utf8_lossy(&response);

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

        let client = self.clone();

        let join: Result<_, Self::Error> = spawn_local(async move {
            let bytes = client.get(url.as_str()).send().await?.body().await?;

            Ok(bytes)
        })
        .await?;
        let response = join?;

        // TODO: convert this to from_utf8_lossy_owned
        let response_text = String::from_utf8_lossy(&response);

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

        let client = self.clone();

        let join: Result<_, Self::Error> = spawn_local(async move {
            let bytes = client.get(url.as_str()).send().await?.body().await?;

            Ok(bytes)
        })
        .await?;
        let response = join?;

        debug!("received response");

        Ok(response.into())
    }
}
