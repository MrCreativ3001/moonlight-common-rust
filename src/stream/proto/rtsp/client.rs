//! A sans io rtsp client implementation with moonlight encryption support

use std::{collections::VecDeque, error::Error, mem::swap, net::SocketAddr, str::Utf8Error};

use thiserror::Error;
use tracing::{Level, debug, instrument, trace};

use crate::{
    crypto::disabled::DisabledCryptoBackend,
    stream::{
        AesKey,
        proto::{
            crypto::CryptoBackend,
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
    #[error("encryption: {0}")]
    Encryption(#[from] RtspEncryptionError),
    #[error("the connection is secured, but no key present")]
    NoEncryptionKey,
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
    #[error("the connection was closed without any payload")]
    Close,
}

#[derive(Debug, PartialEq)]
pub enum RtspOutput {
    Connect { addr: SocketAddr },
    Write { data: Vec<u8> },
    Timeout,
    Response(RtspResponse),
}

#[derive(Debug, PartialEq)]
pub enum RtspInput<'a> {
    Connect,
    Receive(&'a [u8]),
    Disconnect,
}

#[derive(Debug)]
pub struct RtspClientConfig {
    pub target: RtspAddr,
    pub client_version: usize,
    pub aes_key: Option<AesKey>,
}

#[derive(Debug)]
pub struct RtspClient<Crypto> {
    target: RtspAddr,
    client_version: String,
    crypto_backend: Crypto,
    aes_key: Option<AesKey>,
    sequence_number: usize,
    state: State,
    transmit: VecDeque<RtspRequest>,
    current_response: Option<RtspResponse>,
    receive: Vec<u8>,
}

#[derive(Debug)]
enum State {
    Connecting,
    SendRequest,
    WaitResponse,
    Disconnected,
}

impl RtspClient<DisabledCryptoBackend> {
    pub fn new_unencrypted(config: RtspClientConfig) -> Self {
        Self::new(config, DisabledCryptoBackend)
    }
}

/// Sans Io Moonlight Rtsp protocol with encryption support.
impl<Crypto> RtspClient<Crypto>
where
    Crypto: CryptoBackend,
    Crypto::Error: Error + 'static,
{
    // TODO: enet? https://github.com/moonlight-stream/moonlight-common-c/blob/3a377e7d7be7776d68a57828ae22283144285f90/src/RtspConnection.c#L246-L371
    // TODO: maybe make client version an enum?
    #[instrument(level = Level::DEBUG, skip(crypto_backend))]
    pub fn new(mut config: RtspClientConfig, crypto_backend: Crypto) -> Self {
        Self {
            target: config.target,
            crypto_backend,
            aes_key: config.aes_key.take_if(|_| config.target.encrypted),
            client_version: config.client_version.to_string(),
            sequence_number: 1,
            state: State::Disconnected,
            transmit: Default::default(),
            current_response: None,
            receive: Default::default(),
        }
    }

    pub fn target_addr(&self) -> RtspAddr {
        self.target
    }

    pub fn send(&mut self, request: RtspRequest) {
        debug!(request = ?request, "sending rtsp request");
        self.transmit.push_back(request);
    }

    pub fn handle_input(&mut self, input: RtspInput) -> Result<(), RtspClientError> {
        match input {
            RtspInput::Connect => {
                self.state = State::SendRequest;
            }
            RtspInput::Receive(data) => {
                self.receive.extend_from_slice(data);
            }
            RtspInput::Disconnect => {
                let mut receive = Vec::new();
                swap(&mut receive, &mut self.receive);

                trace!("received rtsp response");

                // Decrypt if needed
                let plaintext = if let Some(aes_key) = self.aes_key {
                    let mut plaintext = vec![0; receive.len()];

                    let len = decrypt_server_rtsp_message_into(
                        &self.crypto_backend,
                        aes_key,
                        self.sequence_number,
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
                    && let Ok(response_sequence_number) = response_sequence_number.parse::<usize>()
                {
                    if response_sequence_number == self.sequence_number {
                        self.sequence_number += 1;
                    } else {
                        // TODO: error
                        todo!()
                    }
                } else {
                    // TODO: error
                    todo!()
                }

                // TODO: maybe only look for error codes?
                if response.message.status_code != 200 {
                    return Err(RtspClientError::StatusCode(response.message.status_code));
                }

                let payload = &text[header_len..];
                response.payload = Some(payload.to_owned());

                self.state = State::Disconnected;

                self.receive.clear();
                self.current_response = Some(response);
            }
        }

        Ok(())
    }

    pub fn poll_output(&mut self) -> Result<RtspOutput, RtspClientError> {
        match &self.state {
            State::Connecting => {
                // TODO: close connection because of timeout?
                // todo!();
                Ok(RtspOutput::Timeout)
            }
            State::SendRequest => {
                if let Some(mut request) = self.transmit.pop_front() {
                    // Insert CSeq and Version
                    request
                        .options
                        .push(("CSeq".to_string(), self.sequence_number.to_string()));
                    request.options.push((
                        "X-GS-ClientVersion".to_string(),
                        self.client_version.to_string(),
                    ));
                    request
                        .options
                        .push(("Host".to_string(), self.target.addr.to_string()));
                    // TODO: host?

                    // Send data
                    let plaintext = request.to_string().into_bytes();
                    debug!(plaintext = ?plaintext, "sending raw rtsp request");

                    let data = if self.target.encrypted {
                        let aes_key = self.aes_key.ok_or(RtspClientError::NoEncryptionKey)?;

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

                    self.state = State::WaitResponse;

                    return Ok(RtspOutput::Write { data });
                }

                // TODO: what now? we don't have anything to send
                Ok(RtspOutput::Timeout)
            }
            State::WaitResponse => Ok(RtspOutput::Timeout),
            State::Disconnected => {
                if let Some(current_response) = self.current_response.take() {
                    debug!(response = ?current_response, "received rtsp response");
                    return Ok(RtspOutput::Response(current_response));
                }

                if self.transmit.is_empty() {
                    return Ok(RtspOutput::Timeout);
                }

                self.state = State::Connecting;

                Ok(RtspOutput::Connect {
                    addr: self.target.addr,
                })
            }
        }
    }
}
