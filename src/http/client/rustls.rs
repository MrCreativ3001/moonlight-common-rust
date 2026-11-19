use std::{
    io::{Cursor, Read, Write},
    net::Ipv4Addr,
    str::FromStr,
    sync::Arc,
};

use bytes::Bytes;
use http::{HeaderName, HeaderValue, Request, Response, StatusCode, Uri, header};
use pem::Pem;
use rustls::{
    ClientConfig, ClientConnection, DigitallySignedStruct, RootCertStore,
    client::{
        Resumption, WebPkiServerVerifier,
        danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
    },
    pki_types::{
        CertificateDer, PrivateKeyDer, ServerName, UnixTime,
        pem::{PemObject, SectionKind},
    },
};
use tracing::{debug, trace, warn};

use crate::{
    error::Error,
    http::{
        ClientInfo, Endpoint, QueryBuilder, QueryBuilderError, QueryParam,
        client::InvalidPrivateKey, helper::u32_to_str,
    },
};

#[cfg(test)]
#[path = "rustls_tests.rs"]
mod tests;

fn serialize_request(mut request: Request<Bytes>) -> Vec<u8> {
    let mut buffer = Vec::default();

    // Write first lne
    let _ = write!(
        &mut buffer,
        "{} {} {:?}\r\n",
        request.method(),
        request
            .uri()
            .path_and_query()
            .map(|x| x.as_str())
            .unwrap_or("/"),
        request.version()
    );

    // Write Host
    let _ = write!(
        &mut buffer,
        "Host: {}",
        request.uri().authority().expect("no authority").host()
    );

    // Write content length if necessary
    if !request.body().is_empty() {
        let mut num_buffer = [0; _];
        let num_str = u32_to_str(request.body().len() as u32, &mut num_buffer);

        request.headers_mut().insert(
            header::CONTENT_LENGTH,
            HeaderValue::from_str(num_str).expect("couldn't put u32 into Content-Length header"),
        );
    }

    // Write other headers
    for (name, value) in request.headers() {
        let _ = write!(&mut buffer, "\r\n{name}: ");
        buffer.extend_from_slice(value.as_bytes());
    }

    // Write header end
    let _ = write!(&mut buffer, "\r\n\r\n");

    // Write payload
    buffer.extend_from_slice(request.body());

    buffer
}

fn deserialize_response(buffer: &[u8]) -> Result<Option<Response<Bytes>>, Error> {
    let mut headers = [httparse::EMPTY_HEADER; 64];
    let mut response = httparse::Response::new(&mut headers);
    let httparse::Status::Complete(headers_len) = response.parse(buffer).map_err(Error::other)?
    else {
        return Ok(None);
    };

    let mut content_len = None;
    for header in response.headers.iter() {
        if header.name.eq_ignore_ascii_case("content-length") {
            if let Some(len) = str::from_utf8(header.value)
                .ok()
                .and_then(|value| value.parse::<u32>().ok())
            {
                content_len = Some(len as usize);
            } else {
                warn!(header = ?header, "failed to parse Content-Length header");
            }
        }
    }

    let body = if let Some(content_len) = content_len {
        let Some(content) = buffer.get(headers_len..(headers_len + content_len)) else {
            return Ok(None);
        };

        content
    } else {
        &[]
    };

    // Move from httparse::Response into owned Response
    let mut out = Response::builder();

    if let Some(code) = response.code {
        match StatusCode::from_u16(code) {
            Ok(code) => {
                out = out.status(code);
            }
            Err(err) => {
                warn!(error = %err, status_code = %code, "invalid status code");
            }
        }
    }
    for header in response.headers.iter() {
        let name = match HeaderName::from_str(header.name) {
            Ok(value) => value,
            Err(err) => {
                warn!(error = %err, header_name = ?header.name, "failed to parse header name");
                continue;
            }
        };
        let value = match HeaderValue::from_bytes(header.value) {
            Ok(value) => value,
            Err(err) => {
                warn!(error = %err, header_name = ?name, header_value_raw = ?header.value, "failed to parse header value");
                continue;
            }
        };

        out = out.header(name, value);
    }

    Ok(Some(
        out.body(Bytes::copy_from_slice(body))
            .expect("failed to finish response"),
    ))
}

fn dummy_server_name() -> ServerName<'static> {
    ServerName::IpAddress(rustls::pki_types::IpAddr::V4(
        // We don't care about the ip because hostname / ip validation is disabled
        rustls::pki_types::Ipv4Addr::from(Ipv4Addr::new(0, 0, 0, 0)),
    ))
}

#[derive(Debug, Clone)]
pub struct HttpsConfig {
    config: Option<Arc<ClientConfig>>,
}

impl HttpsConfig {
    pub fn http() -> Self {
        Self { config: None }
    }
    pub fn https(
        client_private_key: &Pem,
        client_certificate: &Pem,
        server_certificate: &Pem,
    ) -> Result<Self, Error> {
        let mut cert_store = RootCertStore::empty();

        let server_certificate =
            CertificateDer::from_slice(server_certificate.contents()).into_owned();
        cert_store.add(server_certificate).map_err(Error::other)?;

        let server_verifier = WebPkiServerVerifier::builder(Arc::new(cert_store))
            .allow_unknown_revocation_status()
            .build()
            .map_err(Error::other)?;
        let server_verifier = NoServerNameVerification::new(server_verifier);

        let client_certificate =
            CertificateDer::from_slice(client_certificate.contents()).into_owned();
        let client_private_key = PrivateKeyDer::from_pem(
            SectionKind::PrivateKey,
            client_private_key.contents().to_vec(),
        )
        .ok_or_else(|| Error::other(InvalidPrivateKey))?;

        let mut config = ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(server_verifier))
            .with_client_auth_cert(vec![client_certificate], client_private_key)
            .map_err(Error::other)?;

        // Disable resumption, Sunshine cannot handle them
        config.resumption = Resumption::disabled();

        Ok(Self {
            config: Some(Arc::new(config)),
        })
    }

    pub fn is_https(&self) -> bool {
        self.config.is_some()
    }
}

/// Simple Sans-IO Http or Https Client
pub struct HttpsClient {
    request_buffer: Vec<u8>,
    response_buffer: Vec<u8>,
    tls: Option<ClientConnection>,
}

impl HttpsClient {
    pub fn new(config: &HttpsConfig, request: Request<Bytes>) -> Result<Self, Error> {
        debug!(request = ?request, "new http client");
        let tls = if let Some(tls_config) = &config.config {
            Some(
                ClientConnection::new(tls_config.clone(), dummy_server_name())
                    .map_err(Error::other)?,
            )
        } else {
            None
        };

        Ok(Self {
            request_buffer: serialize_request(request),
            response_buffer: vec![],
            tls,
        })
    }

    pub fn poll_write(&mut self, buffer: &mut [u8]) -> Result<usize, Error> {
        if let Some(tls) = &mut self.tls {
            // Try to write the plaintext
            let len = tls.writer().write(&self.request_buffer)?;
            self.request_buffer.drain(0..len);

            // Return the ciphertext
            let mut writer = Cursor::new(buffer);
            let len = tls.write_tls(&mut writer).map_err(Error::other)?;

            Ok(len)
        } else {
            let min_len = buffer.len().min(self.request_buffer.len());

            buffer[0..min_len].copy_from_slice(&self.request_buffer[0..min_len]);
            self.request_buffer.drain(0..min_len);

            Ok(min_len)
        }
    }
    pub fn handle_read(&mut self, data: &[u8]) -> Result<(), Error> {
        if let Some(tls) = &mut self.tls {
            let mut reader = Cursor::new(&data);

            let mut written = 0;
            while written < data.len() {
                let len = tls.read_tls(&mut reader)?;
                written += len;

                tls.process_new_packets().map_err(Error::other)?;

                match tls.reader().read_to_end(&mut self.response_buffer) {
                    Ok(_) => {}
                    Err(err) if matches!(err.kind(), std::io::ErrorKind::WouldBlock) => {}
                    Err(err) => return Err(err.into()),
                }
            }

            Ok(())
        } else {
            self.response_buffer.extend_from_slice(data);

            Ok(())
        }
    }

    pub fn poll_output(&mut self) -> Result<Option<Response<Bytes>>, Error> {
        if let Some(tls) = &mut self.tls {
            tls.process_new_packets().map_err(Error::other)?;
        }

        trace!(
            request_buffer = ?String::from_utf8_lossy(&self.request_buffer),
            response_buffer = ?String::from_utf8_lossy(&self.response_buffer),
            "http(s) state"
        );

        let Some(response) = deserialize_response(&self.response_buffer)? else {
            return Ok(None);
        };

        debug!(response = ?response, "http response");

        Ok(Some(response))
    }
}

#[derive(Debug)]
pub struct NoServerNameVerification {
    inner: Arc<WebPkiServerVerifier>,
}

impl NoServerNameVerification {
    pub fn new(inner: Arc<WebPkiServerVerifier>) -> Self {
        Self { inner }
    }
}

impl ServerCertVerifier for NoServerNameVerification {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        match self
            .inner
            .verify_server_cert(_end_entity, _intermediates, _server_name, _ocsp, _now)
        {
            Ok(scv) => Ok(scv),
            Err(rustls::Error::InvalidCertificate(cert_error)) => match cert_error {
                rustls::CertificateError::NotValidForName => Ok(ServerCertVerified::assertion()),
                rustls::CertificateError::NotValidForNameContext { expected, .. }
                    if expected == dummy_server_name() =>
                {
                    Ok(ServerCertVerified::assertion())
                }
                _ => Err(rustls::Error::InvalidCertificate(cert_error)),
            },
            Err(e) => Err(e),
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        self.inner.verify_tls12_signature(message, cert, dss)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        self.inner.verify_tls13_signature(message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.inner.supported_verify_schemes()
    }
}

struct StringQueryBuilder<'a> {
    is_first: bool,
    string: &'a mut String,
}

impl QueryBuilder for StringQueryBuilder<'_> {
    fn append(&mut self, param: QueryParam) -> Result<(), QueryBuilderError> {
        if !self.is_first {
            self.string.push('&');
        }
        self.is_first = false;

        self.string.push_str(param.key);
        self.string.push('=');
        self.string.push_str(param.value);

        Ok(())
    }
}

pub fn build_url<E>(
    use_https: bool,
    client_info: ClientInfo,
    hostport: &str,
    request: &E::Request,
) -> Result<Uri, Error>
where
    E: Endpoint,
{
    let mut path_and_query = format!("{}?", E::path());

    let mut query_builder = StringQueryBuilder {
        is_first: true,
        string: &mut path_and_query,
    };

    // This cannot fail
    #[allow(clippy::unwrap_used)]
    crate::http::Request::append_query_params(&client_info, &mut query_builder).unwrap();
    #[allow(clippy::unwrap_used)]
    crate::http::Request::append_query_params(request, &mut query_builder).unwrap();

    let uri = Uri::builder()
        .scheme(if use_https { "https" } else { "http" })
        .authority(hostport)
        .path_and_query(path_and_query)
        .build()
        .map_err(|err| Error::Other(err.into()))?;

    Ok(uri)
}
