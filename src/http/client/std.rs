use std::{
    io::{Read, Write},
    net::{TcpStream, ToSocketAddrs},
    str::FromStr,
    time::{Duration, Instant},
};

use bytes::Bytes;
use http::Method;
use pem::Pem;
use tracing::{instrument, trace};

use crate::{
    error::Error,
    http::{
        ClientInfo, Endpoint, TextResponse,
        client::{
            DEFAULT_LONG_TIMEOUT, DEFAULT_TIMEOUT,
            blocking_client::RequestClient,
            rustls::{HttpsClient, HttpsConfig, build_url},
        },
    },
};

const RW_BUFFER: usize = 1024;

fn io_err<T>(res: Result<T, std::io::Error>) -> Result<T, Error> {
    match res {
        Ok(value) => Ok(value),
        Err(err)
            if matches!(
                err.kind(),
                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
            ) =>
        {
            Err(Error::ConnectionTimeout)
        }
        Err(err) => Err(err.into()),
    }
}

fn make_request(
    timeout: Duration,
    config: &HttpsConfig,
    request: http::Request<Bytes>,
) -> Result<http::Response<Bytes>, Error> {
    let until = Instant::now() + timeout;
    let max_timeout = || until.saturating_duration_since(Instant::now());

    // Connect via Tcp
    let authority = request.uri().authority().unwrap();

    let mut addrs = authority.as_str().to_socket_addrs()?;

    let Some(addr) = addrs.next() else {
        return Err(
            std::io::Error::other(format!("failed to resolve hostname {authority:?}")).into(),
        );
    };

    trace!(addr = %addr, "tcp connecting...");

    let mut stream = io_err(TcpStream::connect_timeout(&addr, max_timeout()))?;
    drop(addrs);
    trace!("tcp connect");

    // Handle Request
    let mut buffer = [0; RW_BUFFER];

    let mut client = HttpsClient::new(config, request)?;

    let err: Error = 'outer: loop {
        if Instant::now() >= until {
            return Err(Error::ConnectionTimeout);
        }

        // Write data
        'write: loop {
            let len = client.poll_write(&mut buffer);
            if len == 0 {
                break;
            }

            let mut offset = 0;
            loop {
                let timeout = max_timeout();
                if timeout.is_zero() {
                    return Err(Error::ConnectionTimeout);
                }
                let _ = stream.set_write_timeout(Some(timeout));

                offset += match io_err(stream.write(&buffer[offset..len])) {
                    Ok(value) => value,
                    Err(Error::Io(err))
                        if matches!(err.kind(), std::io::ErrorKind::ConnectionReset) =>
                    {
                        break 'write;
                    }
                    Err(err) => return Err(err),
                };
                if offset >= len {
                    break;
                }
            }
        }

        // Read data
        let timeout = max_timeout();
        if timeout.is_zero() {
            return Err(Error::ConnectionTimeout);
        }
        let _ = stream.set_read_timeout(Some(timeout));

        let len = match io_err(stream.read(&mut buffer)) {
            Ok(value) => value,
            Err(Error::Io(err)) if matches!(err.kind(), std::io::ErrorKind::ConnectionReset) => {
                break 'outer err.into();
            }
            Err(err) => return Err(err),
        };
        if len == 0 {
            break 'outer Error::ConnectionFailed;
        }

        client.handle_read(&buffer[0..len]);

        if let Some(response) = client.poll_output()? {
            return Ok(response);
        }
    };

    if let Some(response) = client.poll_output()? {
        Ok(response)
    } else {
        Err(err)
    }
}

#[derive(Debug, Clone)]
pub struct StdClient {
    timeout: Duration,
    config: HttpsConfig,
}

impl RequestClient for StdClient {
    fn with_defaults() -> Result<Self, Error> {
        Ok(Self {
            timeout: DEFAULT_TIMEOUT,
            config: HttpsConfig::http(),
        })
    }
    fn with_defaults_long_timeout() -> Result<Self, Error> {
        Ok(Self {
            timeout: DEFAULT_LONG_TIMEOUT,
            config: HttpsConfig::http(),
        })
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
        Ok(Self {
            timeout: DEFAULT_TIMEOUT,
            config: HttpsConfig::https(client_private_key, client_certificate, server_certificate)?,
        })
    }

    #[instrument(skip(self, request), fields(path = E::path()), err)]
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
        let uri = build_url::<E>(false, client_info, hostport, request)?;

        let request = http::Request::builder()
            .method(Method::GET)
            .uri(uri)
            .body(Bytes::new())
            .expect("failed to create http request");
        let config = HttpsConfig::http();

        let response = make_request(self.timeout, &config, request)?;

        let text = String::from_utf8(response.into_body().into()).map_err(Error::other)?;

        E::Response::from_str(&text)
    }

    #[instrument(skip(self, request), fields(path = E::path()), err)]
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
        let uri = build_url::<E>(false, client_info, hostport, request)?;

        let request = http::Request::builder()
            .method(Method::GET)
            .uri(uri)
            .body(Bytes::new())
            .expect("failed to create http request");

        let response = make_request(self.timeout, &self.config, request)?;

        let text = String::from_utf8(response.into_body().into()).map_err(Error::other)?;

        E::Response::from_str(&text)
    }

    #[instrument(skip(self, request), fields(path = E::path()), err)]
    fn send_https_with_bytes<E>(
        &self,
        client_info: ClientInfo,
        hostport: &str,
        request: &E::Request,
    ) -> Result<E::Response, Error>
    where
        E: Endpoint<Response = Vec<u8>>,
    {
        let uri = build_url::<E>(false, client_info, hostport, request)?;

        let request = http::Request::builder()
            .method(Method::GET)
            .uri(uri)
            .body(Bytes::new())
            .expect("failed to create http request");

        let response = make_request(self.timeout, &self.config, request)?;

        Ok(response.into_body().into())
    }
}
