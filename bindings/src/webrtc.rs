use std::str::FromStr;

use uniffi::{Record, export};

use moonlight_common::webrtc::{MoonlightWebRtcSession as MoonlightWebRtcSession2, Session};

use crate::VideoFormats;

#[derive(Debug, thiserror::Error, Record)]
#[error("{error}")]
pub struct MoonlightSessionParseError {
    pub error: String,
}

#[derive(Debug, Record)]
pub struct MoonlightWebRtcSession {
    pub app_id: u32,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub bitrate: u32,
    pub hdr: bool,
    pub local_audio_play_mode: bool,
    pub preferred_codec: Option<VideoFormats>,
    pub preferred_audio: Option<u32>,
    pub host_id: Option<u32>,
    pub control_simple: bool,
    pub control_enet: bool,
}

impl From<MoonlightWebRtcSession> for MoonlightWebRtcSession2 {
    fn from(value: MoonlightWebRtcSession) -> Self {
        Self {
            app_id: value.app_id,
            width: value.width,
            height: value.height,
            fps: value.fps,
            bitrate: value.bitrate,
            hdr: value.hdr,
            local_audio_play_mode: value.local_audio_play_mode,
            preferred_codec: value.preferred_codec.map(Into::into),
            preferred_audio: value.preferred_audio,
            host_id: value.host_id,
            control_simple: value.control_simple,
            control_enet: value.control_enet,
        }
    }
}
impl From<MoonlightWebRtcSession2> for MoonlightWebRtcSession {
    fn from(value: MoonlightWebRtcSession2) -> Self {
        Self {
            app_id: value.app_id,
            width: value.width,
            height: value.height,
            fps: value.fps,
            bitrate: value.bitrate,
            hdr: value.hdr,
            local_audio_play_mode: value.local_audio_play_mode,
            preferred_codec: value.preferred_codec.map(Into::into),
            preferred_audio: value.preferred_audio,
            host_id: value.host_id,
            control_simple: value.control_simple,
            control_enet: value.control_enet,
        }
    }
}

#[export]
pub fn webrtc_session_parse(
    session: String,
) -> Result<MoonlightWebRtcSession, MoonlightSessionParseError> {
    MoonlightWebRtcSession2::from_str(&session)
        .map(Into::into)
        .map_err(|err| MoonlightSessionParseError {
            error: err.to_string(),
        })
}
#[export]
pub fn webrtc_session_apply(
    session_str: String,
    attributes: MoonlightWebRtcSession,
) -> Result<String, MoonlightSessionParseError> {
    let mut session =
        Session::parse(session_str.as_bytes()).map_err(|err| MoonlightSessionParseError {
            error: err.to_string(),
        })?;

    let attributes = MoonlightWebRtcSession2::from(attributes);
    attributes.apply(&mut session);

    let mut out_session = Vec::new();
    session
        .write(&mut out_session)
        .expect("failed to write session to vec");

    Ok(String::from_utf8_lossy(&out_session).into_owned())
}
