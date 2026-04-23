use crate::{
    ServerVersion,
    stream::proto::{
        packet::SunshinePing,
        rtsp::{
            moonlight::{ParseMoonlightRtspResponseError, RtspSetupRequest, RtspSetupResponse},
            raw::{RtspAddr, RtspRequest, RtspResponse},
        },
    },
};

pub struct RtspSetupFoundationMicRequest {
    #[allow(unused)]
    pub target: RtspAddr,
    pub session_id: Option<String>,
}

impl RtspSetupFoundationMicRequest {
    pub fn into_request(self, server_version: ServerVersion) -> RtspRequest {
        RtspSetupRequest {
            // See
            // https://github.com/qiin2333/moonlight-common-c/blob/7ed14144d1aef1d6d234ea98b17eedc083a5ac36/src/RtspConnection.c#L1419-L1425
            target: if server_version >= ServerVersion::new(5, 0, 0, 0) {
                "streamid=mic/0/0"
            } else {
                "streamid=mic"
            }
            .to_string(),
            session_id: self.session_id,
        }
        .into_request(server_version)
    }
}

#[derive(Debug)]
pub struct RtspSetupFoundationMicResponse {
    pub port: Option<u16>,
    pub session_id: String,
    /// Sunshine extension
    pub sunshine_ping: Option<SunshinePing>,
}

impl RtspSetupFoundationMicResponse {
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
