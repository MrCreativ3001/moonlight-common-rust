//! A sans io rtsp implementation with moonlight encryption support

use std::{collections::VecDeque, error::Error, mem::swap, net::SocketAddr, str::Utf8Error};

use thiserror::Error;
use tracing::{Level, debug, instrument};

use crate::{
    crypto::disabled::DisabledCryptoBackend,
    stream::{
        AesIv, AesKey,
        proto::{
            crypto::CryptoBackend,
            rtsp::{
                encryption::{RtspEncryptionError, encrypt_client_rtsp_message_into},
                packet::RtspEncryptionHeader,
                raw::{
                    ParseRtspResponseError, RtspAddr, RtspAddrParseError, RtspRequest, RtspResponse,
                },
            },
        },
    },
};

mod encryption;
pub mod moonlight;
mod packet;
pub mod raw;

#[allow(clippy::unwrap_used)]
#[cfg(test)]
pub mod test;

#[derive(Debug, Error)]
pub enum RtspError {
    #[error("encryption: {0}")]
    Encryption(#[from] RtspEncryptionError),
    #[error("rtsp addr: {0}")]
    ParseTarget(#[from] RtspAddrParseError),
    #[error("error status code: {0}")]
    StatusCode(u32),
    #[error("failed to parse rtsp response: {0}")]
    Response(#[from] ParseRtspResponseError),
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
    pub encryption: Option<(AesKey, AesIv)>,
}

#[derive(Debug)]
pub struct RtspClient<Crypto> {
    target: RtspAddr,
    client_version: String,
    crypto_backend: Crypto,
    encryption: Option<AesKey>,
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
    WaitPayload,
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
    pub fn new(config: RtspClientConfig, crypto_backend: Crypto) -> Self {
        Self {
            target: config.target,
            crypto_backend,
            encryption: config.encryption.map(|(aes_key, _)| aes_key),
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
        debug!(request = ?request, "Sending Rtsp Request");
        self.transmit.push_back(request);
    }

    pub fn handle_input(&mut self, input: RtspInput) -> Result<(), RtspError> {
        match input {
            RtspInput::Connect => {
                self.state = State::SendRequest;
            }
            RtspInput::Receive(data) => {
                self.receive.extend_from_slice(data);
            }
            RtspInput::Disconnect => {
                if let Some(current_receive) = &mut self.current_response {
                    let mut receive = Vec::new();
                    swap(&mut receive, &mut self.receive);

                    let payload = String::from_utf8(receive).map_err(|err| err.utf8_error())?;

                    if !payload.trim().is_empty() {
                        current_receive.payload = Some(payload);
                    }
                }

                self.state = State::Disconnected;

                self.receive.clear();
            }
        }

        Ok(())
    }

    pub fn poll_output(&mut self) -> Result<RtspOutput, RtspError> {
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

                    let data = if self.target.encrypted {
                        // We are allowed to unwrap because we've already checked for this in the constructor
                        #[allow(clippy::unwrap_used)]
                        let aes_key = self.encryption.unwrap();

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
            State::WaitResponse => {
                let text = str::from_utf8(&self.receive)?;
                if let Some((len, response)) = RtspResponse::try_parse_header(text)? {
                    self.receive.drain(..len);

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
                            // TODO: error
                            todo!()
                        }
                    } else {
                        // TODO: error
                        todo!()
                    }

                    // TODO: maybe only look for error codes?
                    if response.message.status_code != 200 {
                        return Err(RtspError::StatusCode(response.message.status_code));
                    }

                    // Don't submit instantly, ml rtsp protocol doesn't send any indication of content length
                    // the content length will only be known when the connection is closed
                    self.state = State::WaitPayload;
                    self.current_response = Some(response);

                    // We're now waiting for disconnect which will append payload
                    return Ok(RtspOutput::Timeout);
                }

                Ok(RtspOutput::Timeout)
            }
            State::WaitPayload => Ok(RtspOutput::Timeout),
            State::Disconnected => {
                if let Some(current_response) = self.current_response.take() {
                    debug!(response = ?current_response, "Received Rtsp Response");
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
