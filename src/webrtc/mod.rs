//! This module contains common types and functions needed for moonlight game streaming over webrtc.
//! It doesn't contain a full webrtc implementation.
//!
#![doc = include_str!("./protocol.md")]

use sdp_types::{Attribute, ParserError, Session};

pub use sdp_types as sdp;
pub mod answer;
pub mod offer;

#[derive(Debug, thiserror::Error)]
pub enum WebRTCSessionParseError {
    #[error("session parse")]
    Session(#[from] ParserError),
    #[error("missing required attribute: {0}")]
    MissingAttribute(&'static str),
    #[error("invalid integer for {0}: {1}")]
    InvalidInteger(&'static str, String),
    #[error("invalid bool for {0}: {1}")]
    InvalidBool(&'static str, String),
    #[error("invalid mode: {0}")]
    InvalidVideoMode(String),
    #[error("invalid control mode: {0}")]
    InvalidControlMode(String),
}

fn push(session: &mut Session, attribute: impl Into<String>, value: impl Into<String>) {
    session.attributes.push(Attribute {
        attribute: attribute.into(),
        value: Some(value.into()),
    });
}

fn parse_u32(name: &'static str, value: &str) -> Result<u32, WebRTCSessionParseError> {
    value
        .parse()
        .map_err(|_| WebRTCSessionParseError::InvalidInteger(name, value.to_string()))
}

fn parse_bool(name: &'static str, value: &str) -> Result<bool, WebRTCSessionParseError> {
    match value {
        "0" => Ok(false),
        "1" => Ok(true),
        _ => Err(WebRTCSessionParseError::InvalidBool(
            name,
            value.to_string(),
        )),
    }
}

fn bool_str(v: bool) -> &'static str {
    if v { "1" } else { "0" }
}
