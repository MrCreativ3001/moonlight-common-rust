//! A sans io rtsp implementation with moonlight encryption support

use std::{
    collections::VecDeque,
    mem::swap,
    net::{AddrParseError, SocketAddr},
    str::Utf8Error,
};

use thiserror::Error;
use tracing::trace;

use crate::stream::proto::rtsp::raw::{
    ParseRtspResponseError, RtspAddr, RtspRequest, RtspResponse,
};

pub mod encryption;
pub mod moonlight;
pub mod raw;

#[allow(clippy::unwrap_used)]
#[cfg(test)]
pub mod test;

#[derive(Debug, Error, PartialEq)]
pub enum RtspError {
    #[error("unknown rtsp protcol: {0}")]
    UnknownProtocol(String),
    #[error("error status code: {0}")]
    StatusCode(u32),
    #[error("failed to parse rtsp target address: {0}")]
    ParseTarget(#[from] AddrParseError),
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
pub struct Rtsp {
    target: RtspAddr,
    client_version: String,
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

/// Sans Io Moonlight Rtsp protocol with encryption support.
impl Rtsp {
    // TODO: enet? https://github.com/moonlight-stream/moonlight-common-c/blob/3a377e7d7be7776d68a57828ae22283144285f90/src/RtspConnection.c#L246-L371
    // TODO: maybe make client version an enum?
    pub fn new(rtsp_url: &str, client_version: usize) -> Result<Self, RtspError> {
        trace!(target: "moonlight_proto_rtsp", "Rtsp Init: \"{rtsp_url}\" and version: {client_version}");

        let client_version = client_version.to_string();

        let target;

        if let Some(target_in) = rtsp_url.strip_prefix("rtsp://") {
            target = Self::parse_target(false, target_in)?;
        } else if let Some(target_in) = rtsp_url.strip_prefix("rtspenc://") {
            target = Self::parse_target(true, target_in)?;
        } else {
            return Err(RtspError::UnknownProtocol(
                rtsp_url.split(':').next().unwrap_or(rtsp_url).to_string(),
            ));
        }

        Ok(Self {
            target,
            sequence_number: 1,
            client_version,
            state: State::Disconnected,
            transmit: Default::default(),
            current_response: None,
            receive: Default::default(),
        })
    }
    fn parse_target(encrypted: bool, target: &str) -> Result<RtspAddr, RtspError> {
        let addr = target.parse::<SocketAddr>()?;

        Ok(RtspAddr { encrypted, addr })
    }

    pub fn target_addr(&self) -> RtspAddr {
        self.target
    }

    pub fn send(&mut self, request: RtspRequest) {
        trace!(target: "moonlight_proto_rtsp", "Rtsp Send: {request:?}");

        self.transmit.push_back(request);
    }

    pub fn handle_input(&mut self, input: RtspInput) -> Result<(), RtspError> {
        trace!(target: "moonlight_proto_rtsp", "Rtsp Input: {input:?}");

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
        trace!(target: "moonlight_proto_rtsp", "Rtsp State: {:?}", self.state);

        let output = self.do_poll()?;

        trace!(target: "moonlight_proto_rtsp", "Rtsp Output: {output:?}");

        Ok(output)
    }
    fn do_poll(&mut self) -> Result<RtspOutput, RtspError> {
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
                    let data = request.to_string().into_bytes();

                    if self.target.encrypted {
                        // TODO: encryption
                        todo!()
                    }

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
