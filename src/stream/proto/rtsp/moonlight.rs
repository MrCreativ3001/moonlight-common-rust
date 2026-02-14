//! All rtsp messages are here

use std::{num::ParseIntError, ops::Deref, str::FromStr};

use thiserror::Error;

use crate::{
    ServerVersion,
    stream::proto::{
        rtsp::{
            Rtsp, RtspRequest,
            raw::{RtspCommand, RtspProtocol, RtspRequestMessage, RtspResponse},
        },
        sdp::{ParseSdpError, Sdp, client::ClientSdp, server::ServerSdp},
    },
};

pub const DEFAULT_AUDIO_PORT: u16 = 48000;

#[derive(Debug, Error)]
pub enum ParseMoonlightRtspResponseError {
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

pub fn send_rtsp_options(rtsp: &mut Rtsp) {
    let request = RtspRequest {
        message: RtspRequestMessage {
            command: RtspCommand::Options,
            target: rtsp.target_addr().to_string(),
            protocol: RtspProtocol::V1_0,
        },
        options: vec![],
        payload: None,
    };

    rtsp.send(request);
}

// TODO: check for values in the response that they are what they say

#[derive(Debug)]
pub struct RtspOptionsResponse {}

impl RtspOptionsResponse {
    pub fn try_from_response(
        response: &RtspResponse,
    ) -> Result<RtspOptionsResponse, ParseMoonlightRtspResponseError> {
        let _ = response;

        Ok(RtspOptionsResponse {})
    }
}

pub fn send_rtsp_describe(rtsp: &mut Rtsp) {
    let request = RtspRequest {
        message: RtspRequestMessage {
            command: RtspCommand::Describe,
            target: rtsp.target_addr().to_string(),
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
    };

    rtsp.send(request);
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
        let Some(sdp) = &response.payload else {
            return Err(ParseMoonlightRtspResponseError::NoPayload);
        };

        let sdp = Sdp::from_str(sdp)?;
        let sdp = ServerSdp::parse(sdp)?;

        Ok(Self { sdp })
    }
}

fn send_rtsp_setup(rtsp: &mut Rtsp, target: String, session_id: Option<String>) {
    let mut request = RtspRequest {
        message: RtspRequestMessage {
            command: RtspCommand::Setup,
            target,
            protocol: RtspProtocol::V1_0,
        },
        options: vec![
            // TODO: set based on appversionquad: https://github.com/moonlight-stream/moonlight-common-c/blob/b126e481a195fdc7152d211def17190e3434bcce/src/RtspConnection.c#L577
            (
                "Transport".to_string(),
                // It looks like GFE doesn't care what we say our port is but
                // we need to give it some port to successfully complete the
                // handshake process.
                "unicast;X-GS-ClientPort=50000-50001".to_string(),
            ),
            (
                "If-Modified-Since".to_string(),
                "Thu, 01 Jan 1970 00:00:00 GMT".to_string(),
            ),
        ],
        payload: None,
    };

    if let Some(session_id) = session_id {
        request.options.push(("Session".to_string(), session_id));
    }

    rtsp.send(request);
}

#[derive(Debug)]
struct RtspSetupResponse {
    port: Option<u16>,
    session_id: String,
    /// Sunshine extension
    sunshine_ping: Option<SunshinePing>,
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
                        // TODO: log error?
                        Err(err) => None,
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
        // TODO: is the timeout needed? https://github.com/moonlight-stream/moonlight-common-c/blob/b126e481a195fdc7152d211def17190e3434bcce/src/RtspConnection.c#L1212
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
                // TODO: warn?
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

pub fn send_rtsp_setup_audio(rtsp: &mut Rtsp, session_id: Option<String>) {
    // TODO: set target based on appversionquad: https://github.com/moonlight-stream/moonlight-common-c/blob/b126e481a195fdc7152d211def17190e3434bcce/src/RtspConnection.c#L1161C75-L1161C89

    // TODO: implement this
    // For GFE 3.22 compatibility, we must start the audio ping thread before the RTSP handshake.
    // It will not reply to our RTSP PLAY request until the audio ping has been received.

    send_rtsp_setup(rtsp, "streamid=audio".to_string(), session_id);
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
        let response = RtspSetupResponse::try_from_response(response)?;

        Ok(Self {
            port: response.port,
            session_id: response.session_id,
            sunshine_ping: response.sunshine_ping,
        })
    }
}

pub fn send_rtsp_video_setup(
    rtsp: &mut Rtsp,
    server_version: ServerVersion,
    session_id: Option<String>,
) {
    // set based target on version quad: https://github.com/moonlight-stream/moonlight-common-c/blob/b126e481a195fdc7152d211def17190e3434bcce/src/RtspConnection.c#L1229
    let target = if server_version.major >= 5 {
        "streamid=video/0/0"
    } else {
        "streamid=video"
    };

    send_rtsp_setup(rtsp, target.to_string(), session_id);
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
        let response = RtspSetupResponse::try_from_response(response)?;

        Ok(Self {
            port: response.port,
            session_id: response.session_id,
            sunshine_ping: response.sunshine_ping,
        })
    }
}

pub fn send_rtsp_control_setup(rtsp: &mut Rtsp, session_id: Option<String>) {
    // TODO: set target based on versionquad: https://github.com/moonlight-stream/moonlight-common-c/blob/b126e481a195fdc7152d211def17190e3434bcce/src/RtspConnection.c#L939

    send_rtsp_setup(rtsp, "stream=control/13/0".to_string(), session_id);
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

pub fn send_rtsp_announce(rtsp: &mut Rtsp, session_id: String, sdp: ClientSdp) {
    // TODO: set target based on versionquad: https://github.com/moonlight-stream/moonlight-common-c/blob/b126e481a195fdc7152d211def17190e3434bcce/src/RtspConnection.c#L633

    // TODO: generate sdp: https://github.com/moonlight-stream/moonlight-common-c/blob/master/src/SdpGenerator.c#L566

    let request = RtspRequest {
        message: RtspRequestMessage {
            command: RtspCommand::Announce,
            target: "streamid=control".to_string(),
            protocol: RtspProtocol::V1_0,
        },
        options: vec![
            ("Session".to_string(), session_id),
            ("Content-Type".to_string(), "application/sdp".to_string()),
        ],
        payload: Some(format!("{}", sdp.into_sdp())),
    };

    rtsp.send(request);
}

// TODO: https://github.com/moonlight-stream/moonlight-common-c/blob/3a377e7d7be7776d68a57828ae22283144285f90/src/RtspConnection.c#L1330-L1390
pub fn send_rtsp_play(rtsp: &mut Rtsp, session_id: String) {
    let request = RtspRequest {
        message: RtspRequestMessage {
            command: RtspCommand::Play,
            target: "/".to_string(),
            protocol: RtspProtocol::V1_0,
        },
        options: vec![("Session".to_string(), session_id)],
        payload: None,
    };

    rtsp.send(request);
}

// TODO: where is this used?
// https://github.com/moonlight-stream/moonlight-common-c/blob/b126e481a195fdc7152d211def17190e3434bcce/src/Video.h#L48
// TODO: maybe don't make this a const?
pub const SUNSHINE_PING_PAYLOAD_SIZE: usize = 16;

#[derive(Debug, Clone)]
pub struct SunshinePing(pub [u8; SUNSHINE_PING_PAYLOAD_SIZE]);

impl Deref for SunshinePing {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
