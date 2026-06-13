use std::fmt;
use std::str::FromStr;

use sdp_types::Session;

use crate::stream::video::VideoFormats;
use crate::webrtc::{WebRTCSessionParseError, bool_str, parse_bool, parse_u32, push};

const CONTROL_MODE_SIMPLE: &str = "simple";
const CONTROL_MODE_ENET: &str = "enet";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlMode {
    Simple,
    Enet,
}

impl fmt::Display for ControlMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ControlMode::Simple => write!(f, "{CONTROL_MODE_SIMPLE}"),
            ControlMode::Enet => write!(f, "{CONTROL_MODE_ENET}"),
        }
    }
}

impl FromStr for ControlMode {
    type Err = WebRTCSessionParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            CONTROL_MODE_SIMPLE => Ok(Self::Simple),
            CONTROL_MODE_ENET => Ok(Self::Enet),
            _ => Err(WebRTCSessionParseError::InvalidControlMode(
                value.to_string(),
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct WebRTCSessionOffer {
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

impl FromStr for WebRTCSessionOffer {
    type Err = WebRTCSessionParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let session = Session::parse(s.as_bytes())?;

        Self::from_sdp(&session)
    }
}

impl WebRTCSessionOffer {
    pub fn from_sdp(session: &Session) -> Result<Self, WebRTCSessionParseError> {
        let mut app_id = None;

        // All are parsed in the same statement -> only need one option
        let mut width = None;
        let mut height = 0;
        let mut fps = 0;

        let mut bitrate = None;

        let mut hdr = false;
        let mut local_audio_play_mode = true;
        let mut preferred_codec = None;
        let mut preferred_audio = None;
        let mut host_id = None;

        let mut control_simple = false;
        let mut control_enet = false;

        for attr in &session.attributes {
            let Some(value) = &attr.value else {
                continue;
            };

            match attr.attribute.as_str() {
                "x-moonlight-app-id" => {
                    app_id = Some(parse_u32("x-moonlight-app-id", value)?);
                }
                "x-moonlight-mode" => {
                    let mut parts = value.split('x');

                    width = Some(
                        parts
                            .next()
                            .ok_or(WebRTCSessionParseError::InvalidVideoMode(
                                "missing width".to_string(),
                            ))?
                            .parse::<u32>()
                            .map_err(|_| {
                                WebRTCSessionParseError::InvalidVideoMode(value.to_string())
                            })?,
                    );

                    height = parts
                        .next()
                        .ok_or(WebRTCSessionParseError::InvalidVideoMode(
                            "missing height".to_string(),
                        ))?
                        .parse::<u32>()
                        .map_err(|err| {
                            WebRTCSessionParseError::InvalidVideoMode(err.to_string())
                        })?;

                    fps = parts
                        .next()
                        .ok_or(WebRTCSessionParseError::InvalidVideoMode(
                            "missing fps".to_string(),
                        ))?
                        .parse::<u32>()
                        .map_err(|err| {
                            WebRTCSessionParseError::InvalidVideoMode(err.to_string())
                        })?;
                }
                "x-moonlight-bitrate" => {
                    bitrate = Some(parse_u32("x-moonlight-bitrate", value)?);
                }

                "x-moonlight-hdr" => {
                    hdr = parse_bool("x-moonlight-hdr", value)?;
                }

                "x-moonlight-local-audio-play-mode" => {
                    local_audio_play_mode = parse_bool("x-moonlight-local-audio-play-mode", value)?;
                }

                "x-moonlight-preferred-codec" => {
                    let value = parse_u32("x-moonlight-preferred-codec", value)?;

                    preferred_codec = Some(VideoFormats::from_bits_retain(value));
                }

                "x-moonlight-preferred-audio" => {
                    preferred_audio = Some(parse_u32("x-moonlight-preferred-audio", value)?);
                }

                "x-moonlight-host-id" => {
                    host_id = Some(parse_u32("x-moonlight-host-id", value)?);
                }

                "x-moonlight-control" => {
                    let mode = ControlMode::from_str(value)?;
                    match mode {
                        ControlMode::Simple => control_simple = true,
                        ControlMode::Enet => control_enet = true,
                    }
                }

                _ => {}
            }
        }

        Ok(Self {
            app_id: app_id.ok_or(WebRTCSessionParseError::MissingAttribute(
                "x-moonlight-app-id",
            ))?,
            width: width.ok_or(WebRTCSessionParseError::MissingAttribute(
                "x-moonlight-mode",
            ))?,
            height,
            fps,
            bitrate: bitrate.ok_or(WebRTCSessionParseError::MissingAttribute(
                "x-moonlight-bitrate",
            ))?,
            hdr,
            local_audio_play_mode,
            preferred_codec,
            preferred_audio,
            host_id,
            control_simple,
            control_enet,
        })
    }

    pub fn apply(&self, session: &mut Session) {
        push(session, "x-moonlight-app-id", self.app_id.to_string());

        push(
            session,
            "x-moonlight-mode",
            format!("{}x{}x{}", self.width, self.height, self.fps),
        );

        push(session, "x-moonlight-bitrate", self.bitrate.to_string());

        push(session, "x-moonlight-hdr", bool_str(self.hdr));

        push(
            session,
            "x-moonlight-local-audio-play-mode",
            bool_str(self.local_audio_play_mode),
        );

        if let Some(v) = self.preferred_codec {
            push(session, "x-moonlight-preferred-codec", v.bits().to_string());
        }

        if let Some(v) = self.preferred_audio {
            push(session, "x-moonlight-preferred-audio", v.to_string());
        }

        if let Some(v) = self.host_id {
            push(session, "x-moonlight-host-id", v.to_string());
        }

        if self.control_simple {
            push(session, "x-moonlight-control", CONTROL_MODE_SIMPLE);
        }
        if self.control_enet {
            push(session, "x-moonlight-control", CONTROL_MODE_ENET);
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    use sdp_types::{Attribute, Origin, Session};

    fn session() -> Session {
        Session {
            origin: Origin {
                username: Some("-".into()),
                sess_id: "0".into(),
                sess_version: 0,
                nettype: "IN".into(),
                addrtype: "IP4".into(),
                unicast_address: "127.0.0.1".into(),
            },

            session_name: "-".into(),

            session_description: None,
            uri: None,

            emails: vec![],
            phones: vec![],

            connection: None,
            bandwidths: vec![],
            times: vec![],
            time_zones: vec![],

            key: None,

            attributes: vec![],
            medias: vec![],
        }
    }

    fn attr(name: &str, value: &str) -> Attribute {
        Attribute {
            attribute: name.into(),
            value: Some(value.into()),
        }
    }

    #[test]
    fn parse_session() {
        let mut sdp = session();

        sdp.attributes.push(attr("x-moonlight-app-id", "12345"));

        sdp.attributes
            .push(attr("x-moonlight-mode", "1920x1080x60"));

        sdp.attributes.push(attr("x-moonlight-bitrate", "20000"));

        sdp.attributes.push(attr("x-moonlight-hdr", "1"));

        sdp.attributes.push(attr("x-moonlight-control", "enet"));

        let parsed = WebRTCSessionOffer::from_sdp(&sdp).unwrap();

        assert_eq!(parsed.app_id, 12345);

        assert_eq!(parsed.width, 1920);
        assert_eq!(parsed.height, 1080);
        assert_eq!(parsed.fps, 60);

        assert_eq!(parsed.bitrate, 20000);

        assert!(parsed.hdr);

        assert!(!parsed.control_simple);
        assert!(parsed.control_enet);
    }

    #[test]
    fn serialize_roundtrip() {
        let original = WebRTCSessionOffer {
            app_id: 1,
            width: 2560,
            height: 1440,
            fps: 120,
            bitrate: 50000,
            hdr: true,
            local_audio_play_mode: false,
            preferred_codec: Some(VideoFormats::H264),
            preferred_audio: Some(2),
            host_id: Some(42),
            control_simple: false,
            control_enet: true,
        };

        let mut sdp = session();

        original.apply(&mut sdp);

        let parsed = WebRTCSessionOffer::from_sdp(&sdp).unwrap();

        assert_eq!(original, parsed);
    }

    #[test]
    fn invalid_bool_fails() {
        let mut sdp = session();

        sdp.attributes.push(attr("x-moonlight-app-id", "1"));

        sdp.attributes
            .push(attr("x-moonlight-mode", "1920x1080x60"));

        sdp.attributes.push(attr("x-moonlight-bitrate", "10000"));

        sdp.attributes.push(attr("x-moonlight-hdr", "true"));

        assert!(WebRTCSessionOffer::from_sdp(&sdp).is_err());
    }

    #[test]
    fn missing_required_attribute_fails() {
        let sdp = session();

        assert!(WebRTCSessionOffer::from_sdp(&sdp).is_err());
    }
}
