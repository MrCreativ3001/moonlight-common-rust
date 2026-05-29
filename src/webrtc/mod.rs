//! This module contains common types and functions needed for moonlight game streaming over webrtc.
//! It doesn't contain a full webrtc implementation.
//!
#![doc = include_str!("./protocol.md")]

use std::fmt;
use std::str::FromStr;

use sdp_types::{Attribute, Session};

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
    type Err = MoonlightError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            CONTROL_MODE_SIMPLE => Ok(Self::Simple),
            CONTROL_MODE_ENET => Ok(Self::Enet),
            _ => Err(MoonlightError::InvalidControlMode(value.to_string())),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoMode {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
}

impl fmt::Display for VideoMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}x{}x{}", self.width, self.height, self.fps)
    }
}

impl FromStr for VideoMode {
    type Err = MoonlightError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let parts: Vec<_> = value.split('x').collect();

        if parts.len() != 3 {
            return Err(MoonlightError::InvalidVideoMode(value.to_string()));
        }

        Ok(Self {
            width: parts[0]
                .parse()
                .map_err(|_| MoonlightError::InvalidVideoMode(value.to_string()))?,

            height: parts[1]
                .parse()
                .map_err(|_| MoonlightError::InvalidVideoMode(value.to_string()))?,

            fps: parts[2]
                .parse()
                .map_err(|_| MoonlightError::InvalidVideoMode(value.to_string()))?,
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum MoonlightError {
    #[error("missing required attribute: {0}")]
    MissingAttribute(&'static str),

    #[error("invalid integer for {0}: {1}")]
    InvalidInteger(&'static str, String),

    #[error("invalid bool for {0}: {1}")]
    InvalidBool(&'static str, String),

    #[error("invalid video mode: {0}")]
    InvalidVideoMode(String),

    #[error("invalid control mode: {0}")]
    InvalidControlMode(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoonlightWebRtcSession {
    pub app_id: u32,
    pub mode: VideoMode,
    pub bitrate: u32,
    pub hdr: bool,
    pub local_audio_play_mode: bool,
    pub preferred_codec: Option<u32>,
    pub preferred_audio: Option<u32>,
    pub host_id: Option<u32>,
    pub control_simple: bool,
    pub control_enet: bool,
}

impl MoonlightWebRtcSession {
    pub fn from_sdp(session: &Session) -> Result<Self, MoonlightError> {
        let mut app_id = None;
        let mut mode = None;
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
                "x-moonlight-appid" => {
                    app_id = Some(parse_u32("x-moonlight-appid", value)?);
                }

                "x-moonlight-mode" => {
                    mode = Some(VideoMode::from_str(value)?);
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
                    preferred_codec = Some(parse_u32("x-moonlight-preferred-codec", value)?);
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
            app_id: app_id.ok_or(MoonlightError::MissingAttribute("x-moonlight-appid"))?,
            mode: mode.ok_or(MoonlightError::MissingAttribute("x-moonlight-mode"))?,
            bitrate: bitrate.ok_or(MoonlightError::MissingAttribute("x-moonlight-bitrate"))?,
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
        push(session, "x-moonlight-appid", self.app_id.to_string());

        push(session, "x-moonlight-mode", self.mode.to_string());

        push(session, "x-moonlight-bitrate", self.bitrate.to_string());

        push(session, "x-moonlight-hdr", bool_str(self.hdr));

        push(
            session,
            "x-moonlight-local-audio-play-mode",
            bool_str(self.local_audio_play_mode),
        );

        if let Some(v) = self.preferred_codec {
            push(session, "x-moonlight-preferred-codec", v.to_string());
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

fn push(session: &mut Session, attribute: impl Into<String>, value: impl Into<String>) {
    session.attributes.push(Attribute {
        attribute: attribute.into(),
        value: Some(value.into()),
    });
}

fn parse_u32(name: &'static str, value: &str) -> Result<u32, MoonlightError> {
    value
        .parse()
        .map_err(|_| MoonlightError::InvalidInteger(name, value.to_string()))
}

fn parse_bool(name: &'static str, value: &str) -> Result<bool, MoonlightError> {
    match value {
        "0" => Ok(false),
        "1" => Ok(true),
        _ => Err(MoonlightError::InvalidBool(name, value.to_string())),
    }
}

fn bool_str(v: bool) -> &'static str {
    if v { "1" } else { "0" }
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

        sdp.attributes.push(attr("x-moonlight-appid", "12345"));

        sdp.attributes
            .push(attr("x-moonlight-mode", "1920x1080x60"));

        sdp.attributes.push(attr("x-moonlight-bitrate", "20000"));

        sdp.attributes.push(attr("x-moonlight-hdr", "1"));

        sdp.attributes.push(attr("x-moonlight-control", "enet"));

        let parsed = MoonlightWebRtcSession::from_sdp(&sdp).unwrap();

        assert_eq!(parsed.app_id, 12345);

        assert_eq!(
            parsed.mode,
            VideoMode {
                width: 1920,
                height: 1080,
                fps: 60,
            }
        );

        assert_eq!(parsed.bitrate, 20000);

        assert!(parsed.hdr);

        assert!(!parsed.control_simple);
        assert!(parsed.control_enet);
    }

    #[test]
    fn serialize_roundtrip() {
        let original = MoonlightWebRtcSession {
            app_id: 1,
            mode: VideoMode {
                width: 2560,
                height: 1440,
                fps: 120,
            },
            bitrate: 50000,
            hdr: true,
            local_audio_play_mode: false,
            preferred_codec: Some(7),
            preferred_audio: Some(2),
            host_id: Some(42),
            control_simple: false,
            control_enet: true,
        };

        let mut sdp = session();

        original.apply(&mut sdp);

        let parsed = MoonlightWebRtcSession::from_sdp(&sdp).unwrap();

        assert_eq!(original, parsed);
    }

    #[test]
    fn invalid_mode_fails() {
        assert!(VideoMode::from_str("1920x1080").is_err());
    }

    #[test]
    fn invalid_bool_fails() {
        let mut sdp = session();

        sdp.attributes.push(attr("x-moonlight-appid", "1"));

        sdp.attributes
            .push(attr("x-moonlight-mode", "1920x1080x60"));

        sdp.attributes.push(attr("x-moonlight-bitrate", "10000"));

        sdp.attributes.push(attr("x-moonlight-hdr", "true"));

        assert!(MoonlightWebRtcSession::from_sdp(&sdp).is_err());
    }

    #[test]
    fn missing_required_attribute_fails() {
        let sdp = session();

        assert!(MoonlightWebRtcSession::from_sdp(&sdp).is_err());
    }
}
