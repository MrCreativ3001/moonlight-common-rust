use std::{
    fmt::{self, Display, Formatter},
    net::{AddrParseError, SocketAddr},
    num::ParseIntError,
    str::FromStr,
};
use thiserror::Error;

use crate::stream::proto::rtsp::RtspError;

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum RtspCommand {
    Options,
    Describe,
    Setup,
    Announce,
    Play,
}

impl Display for RtspCommand {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let str = match self {
            Self::Options => "OPTIONS",
            Self::Describe => "DESCRIBE",
            Self::Setup => "SETUP",
            Self::Announce => "ANNOUNCE",
            Self::Play => "PLAY",
        };

        write!(f, "{}", str)
    }
}

#[derive(Clone, Debug, Error, PartialEq)]
pub enum RtspAddrParseError {
    #[error("unknown rtsp protocol: {0}")]
    UnknownProtocol(String),
    #[error("addr: {0}")]
    Addr(#[from] AddrParseError),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RtspAddr {
    pub encrypted: bool,
    pub addr: SocketAddr,
}

impl FromStr for RtspAddr {
    type Err = RtspAddrParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if let Some(target_in) = s.strip_prefix("rtsp://") {
            let addr = target_in.parse()?;

            Ok(RtspAddr {
                encrypted: false,
                addr,
            })
        } else if let Some(target_in) = s.strip_prefix("rtspenc://") {
            let addr = target_in.parse()?;

            Ok(RtspAddr {
                encrypted: true,
                addr,
            })
        } else {
            Err(RtspAddrParseError::UnknownProtocol(
                s.split(':').next().unwrap_or(s).to_string(),
            ))
        }
    }
}

impl Display for RtspAddr {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let protocol = if !self.encrypted { "rtsp" } else { "rtspenc" };
        write!(f, "{protocol}://{}", self.addr)
    }
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum RtspProtocol {
    /// RTSP/1.0
    V1_0,
}

impl Display for RtspProtocol {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let str = match self {
            Self::V1_0 => "RTSP/1.0",
        };

        write!(f, "{}", str)
    }
}

#[derive(Debug, Error, PartialEq)]
pub enum ParseRtspProtocolError {
    #[error("invalid or unknown rtsp protocol: {0}")]
    InvalidProtocol(String),
}

impl FromStr for RtspProtocol {
    type Err = ParseRtspProtocolError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "RTSP/1.0" => Ok(Self::V1_0),
            unknown => Err(ParseRtspProtocolError::InvalidProtocol(unknown.to_string())),
        }
    }
}

#[derive(Debug, Clone)]
pub struct RtspRequestMessage {
    pub command: RtspCommand,
    pub target: String,
    pub protocol: RtspProtocol,
}

impl Display for RtspRequestMessage {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{} {} {}", self.command, self.target, self.protocol)
    }
}

#[derive(Debug, Clone)]
pub struct RtspRequest {
    pub message: RtspRequestMessage,
    pub options: Vec<(String, String)>,
    pub payload: Option<String>,
}

impl Display for RtspRequest {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}\r\n", self.message)?;

        debug_assert!(
            self.options.iter().any(|(key, _)| key == "CSeq"),
            "a rtsp message must contain a CSeq"
        );
        for (key, value) in &self.options {
            assert!(
                !(key.contains("\r") || key.contains("\n")),
                "rtsp option keys must not contain a \\r or \\n"
            );
            assert!(
                !(value.contains("\r") || value.contains("\n")),
                "rtsp option values must not contain a \\r or \\n"
            );

            write!(f, "{}: {}\r\n", key.trim(), value.trim())?;
        }

        if let Some(payload) = self.payload.as_ref() {
            write!(f, "Content-Length: {}\r\n", payload.len())?;

            write!(f, "\r\n{}", payload)?;
        } else {
            write!(f, "\r\n")?;
        }

        Ok(())
    }
}

#[derive(Debug, PartialEq)]
pub struct RtspResponseMessage {
    pub protocol: RtspProtocol,
    /// HTTP specification status code
    pub status_code: u32,
    /// HTTP specification status message
    pub status_message: String,
}

impl Display for RtspResponseMessage {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        assert!(
            !(self.status_message.contains("\r") || self.status_message.contains("\n")),
            "The status message must not contain \"\\r\" or \"\\n\""
        );

        write!(
            f,
            "{} {} {}",
            self.protocol, self.status_code, self.status_message
        )
    }
}

#[derive(Debug, Error, PartialEq)]
pub enum ParseRtspResponseMessage {
    #[error("rtsp response message missing protocol")]
    MissingProtocol,
    #[error("rtsp response message: {0}")]
    ParseProtocol(#[from] ParseRtspProtocolError),
    #[error("rtsp response message missing status code")]
    MissingStatusCode,
    #[error("rtsp response message: {0}")]
    ParseStatusCode(#[from] ParseIntError),
    #[error("rtsp response message missing status message")]
    MissingStatusMessage,
}

impl FromStr for RtspResponseMessage {
    type Err = ParseRtspResponseMessage;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut split = s.splitn(3, " ");

        let protocol = RtspProtocol::from_str(
            split
                .next()
                .ok_or(ParseRtspResponseMessage::MissingProtocol)?,
        )?;
        let status_code = u32::from_str(
            split
                .next()
                .ok_or(ParseRtspResponseMessage::MissingStatusCode)?,
        )?;
        let status_message = split
            .next()
            .ok_or(ParseRtspResponseMessage::MissingStatusMessage)?;

        Ok(Self {
            protocol,
            status_code,
            status_message: status_message.to_string(),
        })
    }
}

#[derive(Debug, PartialEq)]
pub struct RtspResponse {
    pub message: RtspResponseMessage,
    // TODO: make this RtspOption
    pub options: Vec<(String, String)>,
    pub payload: Option<String>,
}

#[derive(Debug, Error, PartialEq)]
pub enum ParseRtspResponseError {
    #[error("rtsp response missing message")]
    MissingMessage,
    #[error("rtsp response: {0}")]
    ParseMessage(#[from] ParseRtspResponseMessage),
    #[error("rtsp response has an invalid option: \"{0}\"")]
    InvalidOptionEntry(String),
    #[error("rtsp response has an invalid Content-Length option")]
    InvalidContentLength(ParseIntError),
    #[error("rtsp response has the option \"Content-Length\" two times")]
    ContentLengthTwice,
}

impl RtspResponse {
    /// Important: try parse will only parse until the header end "\r\n\r\n" because moonlight server doesn't send any way to determine the length of a payload
    pub fn try_parse_header(text: &str) -> Result<Option<(usize, RtspResponse)>, RtspError> {
        let Some(header_end) = Self::find_double_crlf(text) else {
            return Ok(None);
        };

        let head = &text[..header_end];

        let mut lines = head.split("\r\n");

        // parse message
        let Some(message) = lines.next() else {
            return Err(RtspError::Response(ParseRtspResponseError::MissingMessage));
        };
        let message =
            RtspResponseMessage::from_str(message).map_err(ParseRtspResponseError::from)?;

        // parse options
        let mut options = Vec::new();

        for line in lines {
            let Some((key, value)) = line.split_once(':') else {
                return Err(RtspError::Response(
                    ParseRtspResponseError::InvalidOptionEntry(line.to_string()),
                ));
            };
            let (key, value) = (key.trim(), value.trim());

            options.push((key.to_string(), value.to_string()));
        }

        Ok(Some((
            head.len() + 4,
            RtspResponse {
                message,
                options,
                payload: None,
            },
        )))
    }

    fn find_double_crlf(buf: &str) -> Option<usize> {
        buf.as_bytes().windows(4).position(|w| w == b"\r\n\r\n")
    }
}

impl Display for RtspResponse {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}\r\n", self.message)?;

        debug_assert!(
            self.options.iter().any(|(key, _)| key == "CSeq"),
            "a rtsp response must contain a CSeq"
        );
        for (key, value) in &self.options {
            assert!(
                !(key.contains("\r") || key.contains("\n")),
                "rtsp option keys must not contain a \\r or \\n"
            );
            assert!(
                !(value.contains("\r") || value.contains("\n")),
                "rtsp option values must not contain a \\r or \\n"
            );

            write!(f, "{}: {}\r\n", key.trim(), value.trim())?;
        }

        write!(f, "\r\n")?;

        if let Some(payload) = &self.payload {
            write!(f, "{}", payload)?;
        }

        Ok(())
    }
}
