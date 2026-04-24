//! All rtsp messages are here

use std::{num::ParseIntError, str::FromStr};

use thiserror::Error;
use tracing::warn;

use crate::{
    ServerVersion,
    stream::proto::{
        packet::{SUNSHINE_PING_PAYLOAD_SIZE, SunshinePing},
        rtsp::raw::{
            RtspAddr, RtspCommand, RtspProtocol, RtspRequest, RtspRequestMessage, RtspResponse,
        },
        sdp::{ParseSdpError, Sdp, client::ClientSdp, server::ServerSdp},
    },
};

// TODO: add tests

pub const DEFAULT_AUDIO_PORT: u16 = 48000;

#[derive(Debug, Error)]
pub enum ParseMoonlightRtspResponseError {
    #[error("status code not success({code}): {message:?}")]
    StatusCode { message: Option<String>, code: i32 },
    #[error("no payload")]
    NoPayload,
    #[error("sdp error: {0}")]
    Sdp(#[from] ParseSdpError),
    #[error("failed to parse int: {0}")]
    ParseInt(#[from] ParseIntError),
    #[error(
        "missing session id, this happens after a stream(e.g. audio/video/control) was setup but no session id was returned by the server"
    )]
    MissingSessionId,
}

pub struct RtspOptionsRequest {
    pub target: RtspAddr,
}

impl RtspOptionsRequest {
    pub fn into_request(self, _server_version: ServerVersion) -> RtspRequest {
        RtspRequest {
            message: RtspRequestMessage {
                command: RtspCommand::Options,
                target: self.target.to_string(),
                protocol: RtspProtocol::V1_0,
            },
            options: vec![],
            payload: None,
        }
    }
}

#[derive(Debug)]
pub struct RtspOptionsResponse {}

impl RtspOptionsResponse {
    pub fn try_from_response(
        response: &RtspResponse,
    ) -> Result<RtspOptionsResponse, ParseMoonlightRtspResponseError> {
        let _ = response;

        if response.message.status_code / 100 != 2 {
            return Err(ParseMoonlightRtspResponseError::StatusCode {
                message: Some(response.message.status_message.clone()),
                code: response.message.status_code as i32,
            });
        }

        Ok(RtspOptionsResponse {})
    }
}

pub struct RtspDescribeRequest {
    pub target: RtspAddr,
}

impl RtspDescribeRequest {
    pub fn into_request(self, _server_version: ServerVersion) -> RtspRequest {
        RtspRequest {
            message: RtspRequestMessage {
                command: RtspCommand::Describe,
                target: self.target.to_string(),
                protocol: RtspProtocol::V1_0,
            },
            options: vec![
                ("Accept".to_string(), "application/sdp".to_string()),
                (
                    "If-Modified-Since".to_string(),
                    "Thu, 01 Jan 1970 00:00:00 GMT".to_string(),
                ),
            ],
            payload: None,
        }
    }
}

// https://github.com/moonlight-stream/moonlight-common-c/blob/b126e481a195fdc7152d211def17190e3434bcce/src/RtspConnection.c#L1057
#[derive(Debug)]
pub struct RtspDescribeResponse {
    pub sdp: ServerSdp,
}

impl RtspDescribeResponse {
    pub fn try_from_response(
        response: &RtspResponse,
    ) -> Result<Self, ParseMoonlightRtspResponseError> {
        if response.message.status_code / 100 != 2 {
            return Err(ParseMoonlightRtspResponseError::StatusCode {
                message: Some(response.message.status_message.clone()),
                code: response.message.status_code as i32,
            });
        }

        let Some(sdp) = &response.payload else {
            return Err(ParseMoonlightRtspResponseError::NoPayload);
        };

        let sdp = Sdp::from_str(sdp)?;
        let sdp = ServerSdp::parse(sdp)?;

        Ok(Self { sdp })
    }
}

pub(crate) struct RtspSetupRequest {
    pub target: String,
    pub session_id: Option<String>,
}

impl RtspSetupRequest {
    pub fn into_request(self, server_version: ServerVersion) -> RtspRequest {
        let mut request = RtspRequest {
            message: RtspRequestMessage {
                command: RtspCommand::Setup,
                target: self.target,
                protocol: RtspProtocol::V1_0,
            },
            options: vec![
                (
                    "Transport".to_string(),
                    if server_version.major >= 6 {
                        // It looks like GFE doesn't care what we say our port is but
                        // we need to give it some port to successfully complete the
                        // handshake process.
                        "unicast;X-GS-ClientPort=50000-50001"
                    } else {
                        " "
                    }
                    .to_string(),
                ),
                (
                    "If-Modified-Since".to_string(),
                    "Thu, 01 Jan 1970 00:00:00 GMT".to_string(),
                ),
            ],
            payload: None,
        };

        if let Some(session_id) = self.session_id {
            request.options.push(("Session".to_string(), session_id));
        }

        request
    }
}

#[derive(Debug)]
pub(crate) struct RtspSetupResponse {
    pub port: Option<u16>,
    pub session_id: String,
    /// Sunshine extension
    pub sunshine_ping: Option<SunshinePing>,
}

impl RtspSetupResponse {
    pub fn try_from_response(
        response: &RtspResponse,
    ) -> Result<RtspSetupResponse, ParseMoonlightRtspResponseError> {
        // Parse the server port from the Transport header
        // Example: unicast;server_port=48000-48001;source=192.168.35.177
        // https://github.com/moonlight-stream/moonlight-common-c/blob/b126e481a195fdc7152d211def17190e3434bcce/src/RtspConnection.c#L705
        let mut port = None;
        if let Some((_, attributes)) = response.options.iter().find(|(key, _)| key == "Transport") {
            for attribute in attributes.split(':') {
                if let Some(value) = attribute.trim().strip_prefix("server_port=") {
                    port = match value.parse::<u16>() {
                        Ok(value) => Some(value),
                        Err(err) => {
                            warn!(error = ?err, "failed to parse port in a audio/video/control stream setup response");

                            None
                        }
                    };
                }
            }
        }

        // Parse session id:
        // Given there is a non-null session id, get the
        // first token of the session until ";", which
        // resolves any 454 session not found errors on
        // standard RTSP server implementations.
        // (i.e - sessionId = "DEADBEEFCAFE;timeout = 90")
        // Timeout doesn't seem to be used
        let session_value = response
            .options
            .iter()
            .find(|(key, _)| key == "Session")
            .map(|(_, value)| value)
            .ok_or(ParseMoonlightRtspResponseError::MissingSessionId)?
            .clone();
        // This unwrap won't panic because it splitn always returns at least on element
        #[allow(clippy::unwrap_used)]
        let session_id = session_value.split(';').next().unwrap().to_string();

        // Parse sunshine ping payload
        // https://github.com/moonlight-stream/moonlight-common-c/blob/b126e481a195fdc7152d211def17190e3434bcce/src/RtspConnection.c#L1187
        let mut sunshine_ping = None;
        if let Some((_, payload_str)) = response
            .options
            .iter()
            .find(|(key, _)| key == "X-SS-Ping-Payload")
        {
            let payload_bytes = payload_str.as_bytes();

            let mut payload = [0; SUNSHINE_PING_PAYLOAD_SIZE];
            if payload_bytes.len() == SUNSHINE_PING_PAYLOAD_SIZE {
                payload.copy_from_slice(&payload_bytes[0..SUNSHINE_PING_PAYLOAD_SIZE]);
            } else {
                warn!(
                    got_ping_payload = ?payload_bytes,
                    got_len = payload_bytes.len(),
                    expected_len = SUNSHINE_PING_PAYLOAD_SIZE,
                    "X-SS-Ping-Payload length invalid"
                );
            }

            sunshine_ping = Some(SunshinePing(payload));
        }

        Ok(RtspSetupResponse {
            port,
            session_id,
            sunshine_ping,
        })
    }
}

pub struct RtspSetupAudioRequest {
    #[allow(unused)]
    pub target: RtspAddr,
    pub session_id: Option<String>,
}

impl RtspSetupAudioRequest {
    pub fn into_request(self, server_version: ServerVersion) -> RtspRequest {
        RtspSetupRequest {
            // See
            // https://github.com/moonlight-stream/moonlight-common-c/blob/b126e481a195fdc7152d211def17190e3434bcce/src/RtspConnection.c#L1160-L1162
            target: if server_version >= ServerVersion::new(5, 0, 0, 0) {
                "streamid=audio/0/0"
            } else {
                "streamid=audio"
            }
            .to_string(),
            session_id: self.session_id,
        }
        .into_request(server_version)
    }
}

#[derive(Debug)]
pub struct RtspSetupAudioResponse {
    pub port: Option<u16>,
    pub session_id: String,
    /// Sunshine extension
    pub sunshine_ping: Option<SunshinePing>,
}

impl RtspSetupAudioResponse {
    pub fn try_from_response(
        response: &RtspResponse,
    ) -> Result<Self, ParseMoonlightRtspResponseError> {
        if response.message.status_code / 100 != 2 {
            return Err(ParseMoonlightRtspResponseError::StatusCode {
                message: Some(response.message.status_message.clone()),
                code: response.message.status_code as i32,
            });
        }

        let response = RtspSetupResponse::try_from_response(response)?;

        Ok(Self {
            port: response.port,
            session_id: response.session_id,
            sunshine_ping: response.sunshine_ping,
        })
    }
}

pub struct RtspSetupVideoRequest {
    #[allow(unused)]
    pub target: RtspAddr,
    pub session_id: Option<String>,
}

impl RtspSetupVideoRequest {
    pub fn into_request(self, server_version: ServerVersion) -> RtspRequest {
        // set based target on version quad: https://github.com/moonlight-stream/moonlight-common-c/blob/b126e481a195fdc7152d211def17190e3434bcce/src/RtspConnection.c#L1229
        let target = if server_version.major >= 5 {
            "streamid=video/0/0"
        } else {
            "streamid=video"
        };

        RtspSetupRequest {
            target: target.to_owned(),
            session_id: self.session_id,
        }
        .into_request(server_version)
    }
}

#[derive(Debug)]
pub struct RtspSetupVideoResponse {
    pub port: Option<u16>,
    pub session_id: String,
    /// Sunshine extension
    pub sunshine_ping: Option<SunshinePing>,
}

impl RtspSetupVideoResponse {
    pub fn try_from_response(
        response: &RtspResponse,
    ) -> Result<Self, ParseMoonlightRtspResponseError> {
        if response.message.status_code / 100 != 2 {
            return Err(ParseMoonlightRtspResponseError::StatusCode {
                message: Some(response.message.status_message.clone()),
                code: response.message.status_code as i32,
            });
        }

        let response = RtspSetupResponse::try_from_response(response)?;

        Ok(Self {
            port: response.port,
            session_id: response.session_id,
            sunshine_ping: response.sunshine_ping,
        })
    }
}

pub struct RtspSetupControlRequest {
    pub session_id: Option<String>,
}

impl RtspSetupControlRequest {
    pub fn into_request(self, server_version: ServerVersion) -> RtspRequest {
        RtspSetupRequest {
            target: "stream=control/13/0".to_string(),
            session_id: self.session_id,
        }
        .into_request(server_version)
    }
}

#[derive(Debug)]
pub struct RtspSetupControlResponse {
    pub port: Option<u16>,
    pub session_id: String,
    /// Sunshine extension
    pub sunshine_connect_data: Option<u32>,
}
impl RtspSetupControlResponse {
    pub fn try_from_response(
        response: &RtspResponse,
    ) -> Result<Self, ParseMoonlightRtspResponseError> {
        if response.message.status_code / 100 != 2 {
            return Err(ParseMoonlightRtspResponseError::StatusCode {
                message: Some(response.message.status_message.clone()),
                code: response.message.status_code as i32,
            });
        }

        let setup = RtspSetupResponse::try_from_response(response)?;

        // Parse the Sunshine control connect data extension if present
        let mut sunshine_connect_data = None;
        if let Some((_, value)) = response
            .options
            .iter()
            .find(|(key, _)| key == "X-SS-Connect-Data")
        {
            sunshine_connect_data = Some(value.parse()?);
        }

        Ok(Self {
            port: setup.port,
            session_id: setup.session_id,
            sunshine_connect_data,
        })
    }
}

pub struct RtspAnnounceRequest {
    pub sdp: ClientSdp,
    pub session_id: String,
}

impl RtspAnnounceRequest {
    pub fn into_request(self, server_version: ServerVersion) -> RtspRequest {
        RtspRequest {
            message: RtspRequestMessage {
                command: RtspCommand::Announce,
                // See
                // https://github.com/moonlight-stream/moonlight-common-c/blob/b126e481a195fdc7152d211def17190e3434bcce/src/RtspConnection.c#L939
                // https://github.com/moonlight-stream/moonlight-common-c/blob/b126e481a195fdc7152d211def17190e3434bcce/src/RtspConnection.c#L633-L634
                target: (if server_version >= ServerVersion::new(7, 1, 431, 0) {
                    "streamid=control/13/0"
                } else {
                    "streamid=video"
                })
                .to_string(),
                protocol: RtspProtocol::V1_0,
            },
            options: vec![
                ("Session".to_string(), self.session_id),
                ("Content-Type".to_string(), "application/sdp".to_string()),
            ],
            payload: Some(format!("{}", self.sdp.into_sdp())),
        }
    }
}

pub struct RtspPlayRequest {
    pub session_id: String,
}

impl RtspPlayRequest {
    pub fn into_request(self, server_version: ServerVersion) -> RtspRequest {
        if server_version.is_nvidia_software() && server_version < ServerVersion::new(7, 1, 431, 0)
        {
            // See
            // https://github.com/moonlight-stream/moonlight-common-c/blob/3a377e7d7be7776d68a57828ae22283144285f90/src/RtspConnection.c#L1330-L1390
            warn!(
                "The stream might not fully start because this implementation doesn't support this version of nvidia game stream."
            );
        }

        RtspRequest {
            message: RtspRequestMessage {
                command: RtspCommand::Play,
                target: "/".to_string(),
                protocol: RtspProtocol::V1_0,
            },
            options: vec![("Session".to_string(), self.session_id)],
            payload: None,
        }
    }
}
