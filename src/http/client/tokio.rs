use std::{str::FromStr, time::Duration};

use bytes::Bytes;
use http::Method;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpStream, lookup_host},
    select,
    time::sleep,
};
use tracing::{Level, instrument};

use crate::{
    error::Error,
    http::{
        ClientInfo, Endpoint, TextResponse,
        client::{
            AuthorityMissing, DEFAULT_LONG_TIMEOUT, DEFAULT_TIMEOUT,
            async_client::RequestClient,
            rustls::{HttpsClient, HttpsConfig, build_url},
        },
    },
};

#[derive(Debug, Clone)]
pub struct TokioClient {
    timeout: Duration,
    config: HttpsConfig,
}

const RW_BUFFER: usize = 1024;

async fn make_request(
    config: &HttpsConfig,
    request: http::Request<Bytes>,
) -> Result<http::Response<Bytes>, Error> {
    // Connect via Tcp
    let Some(authority) = request.uri().authority() else {
        return Err(Error::other(AuthorityMissing));
    };

    let mut addrs = lookup_host(authority.as_str()).await?;

    let Some(addr) = addrs.next() else {
        return Err(
            std::io::Error::other(format!("failed to resolve hostname {authority:?}")).into(),
        );
    };

    let mut stream = TcpStream::connect(addr).await?;
    drop(addrs);

    // Handle Request
    let (mut reader, mut writer) = stream.split();

    let mut read_buffer = [0; RW_BUFFER];

    let mut write_range = 0..0;
    let mut write_buffer = [0; RW_BUFFER];

    let mut client = HttpsClient::new(config, request)?;

    let err = loop {
        if write_range.is_empty() {
            let len = client.poll_write(&mut write_buffer)?;
            write_range = 0..len;
        }

        if let Some(response) = client.poll_output()? {
            return Ok(response);
        }

        select! {
            result = reader.read(&mut read_buffer) => {
                let len = match result {
                    Ok(0) => {
                        break std::io::Error::new(
                            std::io::ErrorKind::UnexpectedEof,
                            "connection closed before response completed",
                        );
                    }
                    Ok(len) => len,
                    Err(err) => break err,
                };

                client.handle_read(&read_buffer[..len])?;
            },

            result = writer.write(&write_buffer[write_range.clone()]),
                if !write_range.is_empty() => {
                let len = match result {
                    Ok(0) => {
                        break std::io::Error::new(
                            std::io::ErrorKind::WriteZero,
                            "failed to write to socket",
                        );
                    }
                    Ok(len) => len,
                    Err(err) => break err,
                };

                write_range = (write_range.start + len)..write_range.end;
            }
        }
    };

    if let Some(response) = client.poll_output()? {
        Ok(response)
    } else {
        Err(err.into())
    }
}

impl RequestClient for TokioClient {
    fn with_defaults_long_timeout() -> Result<Self, Error> {
        Ok(Self {
            timeout: DEFAULT_LONG_TIMEOUT,
            config: HttpsConfig::http(),
        })
    }

    fn with_defaults() -> Result<Self, Error> {
        Ok(Self {
            timeout: DEFAULT_TIMEOUT,
            config: HttpsConfig::http(),
        })
    }

    #[cfg_attr(not(feature = "__tracing_sensitive"), instrument(level = Level::DEBUG, skip_all, err))]
    #[cfg_attr(feature = "__tracing_sensitive", instrument(level = Level::DEBUG, skip_all, err))]
    fn with_certificates(
        client_private_key: &pem::Pem,
        client_certificate: &pem::Pem,
        server_certificate: &pem::Pem,
    ) -> Result<Self, Error> {
        Ok(Self {
            timeout: DEFAULT_TIMEOUT,
            config: HttpsConfig::https(client_private_key, client_certificate, server_certificate)?,
        })
    }

    #[instrument(level = Level::DEBUG, skip(self, request), fields(path = E::path()), err)]
    async fn send_http<E>(
        &self,
        client_info: ClientInfo,
        hostport: &str,
        request: &E::Request,
    ) -> Result<E::Response, Error>
    where
        E: Endpoint,
        E::Request: Sync,
        E::Response: TextResponse,
    {
        let uri = build_url::<E>(false, client_info, hostport, request)?;

        let request = http::Request::builder()
            .method(Method::GET)
            .uri(uri)
            .body(Bytes::new())
            .expect("failed to create http request");
        let config = HttpsConfig::http();

        let response = select! {
            response = make_request(&config, request) => response?,
            _ = sleep(self.timeout) => {
                return Err(Error::ConnectionTimeout);
            }
        };

        let text = String::from_utf8(response.into_body().into()).map_err(Error::other)?;

        E::Response::from_str(&text)
    }

    #[instrument(level = Level::DEBUG, skip(self, request), fields(path = E::path()), err)]
    async fn send_https<E>(
        &self,
        client_info: ClientInfo,
        hostport: &str,
        request: &E::Request,
    ) -> Result<E::Response, Error>
    where
        E: Endpoint,
        E::Request: Sync,
        E::Response: TextResponse,
    {
        if !self.config.is_https() {
            return Err(Error::NotPaired);
        }

        let uri = build_url::<E>(true, client_info, hostport, request)?;

        let request = http::Request::builder()
            .method(Method::GET)
            .uri(uri)
            .body(Bytes::new())
            .expect("failed to create http request");

        let response = select! {
            response = make_request(&self.config, request) => response?,
            _ = sleep(self.timeout) => {
                return Err(Error::ConnectionTimeout);
            }
        };

        let text = String::from_utf8(response.into_body().into()).map_err(Error::other)?;

        E::Response::from_str(&text)
    }

    #[instrument(level = Level::DEBUG, skip(self, request), fields(path = E::path()), err)]
    async fn send_https_with_bytes<E>(
        &self,
        client_info: ClientInfo,
        hostport: &str,
        request: &E::Request,
    ) -> Result<E::Response, Error>
    where
        E: Endpoint<Response = Vec<u8>>,
        E::Request: Sync,
    {
        if !self.config.is_https() {
            return Err(Error::NotPaired);
        }

        let uri = build_url::<E>(true, client_info, hostport, request)?;

        let request = http::Request::builder()
            .method(Method::GET)
            .uri(uri)
            .body(Bytes::new())
            .expect("failed to create http request");

        let response = select! {
            response = make_request(&self.config, request) => response?,
            _ = sleep(self.timeout) => {
                return Err(Error::ConnectionTimeout);
            }
        };

        let body = response.into_body();
        Ok(body.into())
    }
}
