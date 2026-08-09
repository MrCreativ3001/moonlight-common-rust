//! A sans io rtsp client implementation with moonlight encryption support

use std::{mem::swap, net::SocketAddr, str::Utf8Error, sync::Arc};

use thiserror::Error;
use tracing::{Level, debug, instrument, trace, warn};

use crate::{
    crypto::disabled::DisabledCryptoBackend,
    stream::{
        AesKey,
        proto::{
            DynCryptoBackend,
            rtsp::{
                encryption::{
                    RtspEncryptionError, decrypt_server_rtsp_message_into,
                    encrypt_client_rtsp_message_into,
                },
                packet::RtspEncryptionHeader,
                raw::{
                    ParseRtspResponseError, RtspAddr, RtspAddrParseError, RtspRequest, RtspResponse,
                },
            },
        },
    },
};

#[derive(Debug, Error)]
pub enum RtspClientError {
    #[error("cannot queue a new request while another request is currently happening")]
    AlreadySending,
    #[error("encryption: {0}")]
    Encryption(#[from] RtspEncryptionError),
    #[error("the connection is secured, but no key present")]
    MissingEncryptionKey,
    #[error("rtsp addr: {0}")]
    ParseTarget(#[from] RtspAddrParseError),
    #[error("error status code: {0}")]
    StatusCode(u32),
    #[error("failed to parse rtsp response: {0}")]
    Response(#[from] ParseRtspResponseError),
    #[error("received an incomplete rtsp response")]
    IncompleteResponse,
    #[error("failed to convert bytes into utf8")]
    Utf8(#[from] Utf8Error),
    #[error("the response didn't contain a sequence number (CSeq header)")]
    MissingSequenceNumber,
    #[error("the response is not matching the request sequence number")]
    OutOfOrderResponse,
    #[error("the connection was closed without any payload")]
    Close,
}

#[derive(Debug, PartialEq)]
pub enum RtspOutput {
    Connect {
        addr: SocketAddr,
    },
    Write {
        data: Vec<u8>,
    },
    Timeout,
    Response {
        /// This is [None] if [RtspClient::send_no_response] was used, else this will be [Some]
        response: Option<RtspResponse>,
    },
}

#[derive(Debug, PartialEq)]
pub enum RtspInput<'a> {
    /// The TcpStream has received data.
    Receive(&'a [u8]),
    /// The TcpStream has been disconnected by the remote host.
    Disconnected,
}

#[derive(Debug)]
pub struct RtspClientConfig {
    /// This address and the [`rtsp_target`] might not match.
    ///
    /// - Wolf will create fake ip's returned the in the [`LaunchResponse`] to differentiate client rtsp requests.
    pub remote_addr: SocketAddr,
    /// The parsed rtsp_url from the [`LaunchResponse`].
    pub rtsp_target: RtspAddr,
    pub client_version: usize,
    pub aes_key: Option<AesKey>,
}

#[derive(Debug)]
pub struct RtspClient {
    addr: SocketAddr,
    target: RtspAddr,
    client_version: String,
    crypto_backend: DynCryptoBackend,
    aes_key: Option<AesKey>,
    sequence_number: usize,
    state: State,
    // The next request to transmit
    transmit: Option<RtspRequest>,
    // if we expect a response
    expect_response: bool,
    // the current response that'll be returned on next poll
    current_response: Option<RtspResponse>,
    // a temp buffer for the received data
    receive: Vec<u8>,
}

#[derive(Debug)]
enum State {
    /// Waiting to send a request
    WaitForSendRequest,
    SendRequest,
    WaitResponse,
    Disconnected,
}

impl RtspClient {
    #[allow(unused)]
    pub fn new_unencrypted(config: RtspClientConfig) -> Self {
        Self::new(config, Arc::new(DisabledCryptoBackend) as _)
    }
}

/// Sans Io Moonlight Rtsp protocol with encryption support.
impl RtspClient {
    #[instrument(level = Level::DEBUG, skip(crypto_backend))]
    pub fn new(mut config: RtspClientConfig, crypto_backend: DynCryptoBackend) -> Self {
        Self {
            addr: config.remote_addr,
            target: config.rtsp_target,
            crypto_backend,
            aes_key: config.aes_key.take_if(|_| config.rtsp_target.encrypted),
            client_version: config.client_version.to_string(),
            sequence_number: 1,
            state: State::WaitForSendRequest,
            transmit: Default::default(),
            current_response: None,
            expect_response: false,
            receive: Default::default(),
        }
    }

    pub fn target_addr(&self) -> RtspAddr {
        self.target
    }

    pub fn send(&mut self, request: RtspRequest) -> Result<(), RtspClientError> {
        self.send_inner(request)?;
        self.expect_response = true;

        Ok(())
    }
    /// Send a [RtspRequest] without expecting any response.
    pub fn send_no_response(&mut self, request: RtspRequest) -> Result<(), RtspClientError> {
        self.send_inner(request)?;
        self.expect_response = false;

        Ok(())
    }
    fn send_inner(&mut self, request: RtspRequest) -> Result<(), RtspClientError> {
        if self.transmit.is_some() || !matches!(self.state, State::WaitForSendRequest) {
            return Err(RtspClientError::AlreadySending);
        }

        debug!(request = ?request, "sending rtsp request");
        self.transmit = Some(request);

        Ok(())
    }

    pub fn handle_input(&mut self, input: RtspInput) -> Result<(), RtspClientError> {
        match input {
            RtspInput::Receive(data) => {
                self.receive.extend_from_slice(data);
            }
            RtspInput::Disconnected => {
                if self.expect_response {
                    let mut receive = Vec::new();
                    swap(&mut receive, &mut self.receive);

                    trace!(raw_response = ?receive, "received rtsp response bytes");

                    // Decrypt if needed
                    let plaintext = if let Some(aes_key) = self.aes_key {
                        let mut plaintext = vec![0; receive.len()];

                        let len = decrypt_server_rtsp_message_into(
                            &self.crypto_backend,
                            aes_key,
                            &receive,
                            &mut plaintext,
                        )?;

                        plaintext.truncate(len);

                        plaintext
                    } else {
                        receive
                    };

                    let text = str::from_utf8(&plaintext)?;
                    debug!(plaintext = ?text,"received raw rtsp response");

                    // This response doesn't contain the body yet
                    let (header_len, mut response) = RtspResponse::try_parse_header(text)?
                        .ok_or(RtspClientError::IncompleteResponse)?;

                    // check if sequence number matches
                    if let Some((_, response_sequence_number)) = response
                        .options
                        .iter()
                        .find(|(key, _)| key.eq_ignore_ascii_case("CSeq"))
                        && let Ok(response_sequence_number) =
                            response_sequence_number.parse::<usize>()
                    {
                        if response_sequence_number == self.sequence_number {
                            self.sequence_number += 1;
                        } else {
                            return Err(RtspClientError::OutOfOrderResponse);
                        }
                    } else {
                        return Err(RtspClientError::MissingSequenceNumber);
                    }

                    if response.message.status_code / 100 != 2 {
                        return Err(RtspClientError::StatusCode(response.message.status_code));
                    }

                    let payload = &text[header_len..];
                    response.payload = Some(payload.to_owned());

                    self.current_response = Some(response);
                }

                self.state = State::Disconnected;

                self.receive.clear();
            }
        }

        Ok(())
    }

    pub fn poll_output(&mut self) -> Result<RtspOutput, RtspClientError> {
        match &self.state {
            State::WaitForSendRequest => {
                if self.transmit.is_some() {
                    self.state = State::SendRequest;

                    return Ok(RtspOutput::Connect { addr: self.addr });
                }

                // We don't have anything to send
                Ok(RtspOutput::Timeout)
            }
            State::SendRequest => {
                // We can only ever get into this state when we have a request
                #[allow(clippy::unwrap_used)]
                let mut request = self.transmit.take().unwrap();

                // Insert CSeq and Version
                // IMPORTANT: Wolf requires the CSeq to be the first header
                request
                    .options
                    .insert(0, ("CSeq".to_string(), self.sequence_number.to_string()));
                request.options.push((
                    "X-GS-ClientVersion".to_string(),
                    self.client_version.to_string(),
                ));
                request
                    .options
                    .push(("Host".to_string(), self.target.addr.to_string()));

                // Send data
                let plaintext = request.to_string();
                debug!(plaintext = ?plaintext, "sending raw rtsp request");
                let plaintext = plaintext.into_bytes();

                let data = if self.target.encrypted {
                    let aes_key = self.aes_key.ok_or(RtspClientError::MissingEncryptionKey)?;

                    let mut encrypted = vec![0u8; RtspEncryptionHeader::SIZE + plaintext.len()];

                    let len = encrypt_client_rtsp_message_into(
                        &self.crypto_backend,
                        aes_key,
                        self.sequence_number,
                        &plaintext,
                        &mut encrypted,
                    )?;

                    encrypted.truncate(len);

                    encrypted
                } else {
                    plaintext
                };

                self.receive.clear();

                self.state = State::WaitResponse;

                Ok(RtspOutput::Write { data })
            }
            State::WaitResponse => Ok(RtspOutput::Timeout),
            State::Disconnected => {
                self.state = State::WaitForSendRequest;

                if let Some(current_response) = self.current_response.take() {
                    debug!(response = ?current_response, "received rtsp response");

                    Ok(RtspOutput::Response {
                        response: Some(current_response),
                    })
                } else {
                    Ok(RtspOutput::Response { response: None })
                }
            }
        }
    }
}
