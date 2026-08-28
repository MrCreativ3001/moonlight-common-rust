use std::str::FromStr;

use uniffi::{Record, export, remote};

use moonlight_common::webrtc::{
    WebRTCParseError, answer::WebRTCSessionAnswer, header::WebRTCLinkHeader,
    offer::WebRTCSessionOffer as WebRTCSessionOffer2, sdp::Session,
};

use crate::{MoonlightError, VideoFormats};

// -- Offer

#[derive(Debug, Record)]
pub struct WebRTCSessionOffer {
    pub app_id: u32,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub bitrate: u32,
    pub hdr: bool,
    pub local_audio_play_mode: bool,
    pub preferred_codecs: Option<VideoFormats>,
    pub preferred_audio: Option<u32>,
    pub host_id: Option<u32>,
}

impl From<WebRTCSessionOffer> for WebRTCSessionOffer2 {
    fn from(value: WebRTCSessionOffer) -> Self {
        Self {
            app_id: value.app_id,
            width: value.width,
            height: value.height,
            fps: value.fps,
            bitrate: value.bitrate,
            hdr: value.hdr,
            local_audio_play_mode: value.local_audio_play_mode,
            preferred_codecs: value.preferred_codecs.map(Into::into),
            preferred_audio: value.preferred_audio,
            host_id: value.host_id,
        }
    }
}
impl From<WebRTCSessionOffer2> for WebRTCSessionOffer {
    fn from(value: WebRTCSessionOffer2) -> Self {
        Self {
            app_id: value.app_id,
            width: value.width,
            height: value.height,
            fps: value.fps,
            bitrate: value.bitrate,
            hdr: value.hdr,
            local_audio_play_mode: value.local_audio_play_mode,
            preferred_codecs: value.preferred_codecs.map(Into::into),
            preferred_audio: value.preferred_audio,
            host_id: value.host_id,
        }
    }
}

#[export]
pub fn webrtc_session_offer_parse(session: String) -> Result<WebRTCSessionOffer, MoonlightError> {
    let session = WebRTCSessionOffer2::from_str(&session).map(Into::into)?;
    Ok(session)
}
#[export]
pub fn webrtc_session_offer_apply(
    session_str: String,
    attributes: WebRTCSessionOffer,
) -> Result<String, MoonlightError> {
    let mut session = Session::parse(session_str.as_bytes()).map_err(WebRTCParseError::from)?;

    let attributes = WebRTCSessionOffer2::from(attributes);
    attributes.apply(&mut session);

    let mut out_session = Vec::new();
    session
        .write(&mut out_session)
        .expect("failed to write session to vec");

    Ok(String::from_utf8_lossy(&out_session).into_owned())
}

// -- Answer
#[remote(Record)]
pub struct WebRTCSessionAnswer {
    pub app_name: Option<String>,
    pub microphone: bool,
}

#[export]
pub fn webrtc_session_answer_parse(session: String) -> Result<WebRTCSessionAnswer, MoonlightError> {
    let session = WebRTCSessionAnswer::from_str(&session)?;
    Ok(session)
}
#[export]
pub fn webrtc_session_answer_apply(
    session_str: String,
    attributes: WebRTCSessionAnswer,
) -> Result<String, MoonlightError> {
    let mut session = Session::parse(session_str.as_bytes()).map_err(WebRTCParseError::from)?;

    attributes.apply(&mut session);

    let mut out_session = Vec::new();
    session
        .write(&mut out_session)
        .expect("failed to write session to vec");

    Ok(String::from_utf8_lossy(&out_session).into_owned())
}

// -- Headers

#[remote(Enum)]
pub enum WebRTCLinkHeader {
    IceServer {
        url: String,
        username: Option<String>,
        credential: Option<String>,
    },
}

#[export]
pub fn webrtc_link_header_parse(header_value: &str) -> Vec<WebRTCLinkHeader> {
    WebRTCLinkHeader::parse(header_value)
}

pub fn webrtc_link_header_to_string(header: WebRTCLinkHeader) -> String {
    header.to_string()
}
