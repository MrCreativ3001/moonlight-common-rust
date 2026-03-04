use std::{
    cmp::Ordering,
    ffi::NulError,
    fmt::{Debug, Display},
    num::ParseIntError,
    str::FromStr,
};

use thiserror::Error;

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum MoonlightError {
    #[error("couldn't aquire an instance")]
    InstanceAquire,
    #[error("a connection is already active")]
    ConnectionAlreadyExists,
    #[error("the host doesn't support this feature")]
    NotSupportedOnHost,
    #[error("an error happened whilst sending an event")]
    EventSendError(i32),
    #[error("this call requires a GFE version which uses ENet")]
    ENetRequired,
    #[error("a string contained a nul byte which is not allowed in c strings")]
    StringNulError(#[from] NulError),
    #[error("couldn't establish a connection")]
    ConnectionFailed,
    #[error("the client is not paired")]
    NotPaired,
}

pub mod http;
pub mod stream;

pub mod mac;

pub mod crypto;

pub mod high;

#[cfg(test)]
pub(crate) mod test;

#[derive(Debug, Error, Clone, PartialEq)]
#[error("failed to parse the state of the server")]
pub struct ParseServerStateError;

#[derive(Debug, Copy, Clone, PartialEq)]
pub enum ServerState {
    Busy,
    Free,
}

impl ServerState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Free => "SUNSHINE_SERVER_FREE",
            Self::Busy => "SUNSHINE_SERVER_BUSY",
        }
    }
}

impl FromStr for ServerState {
    type Err = ParseServerStateError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            s if s.ends_with("FREE") => Ok(ServerState::Free),
            s if s.ends_with("BUSY") => Ok(ServerState::Busy),
            _ => Err(ParseServerStateError),
        }
    }
}

#[derive(Debug, Error, PartialEq)]
#[error("failed to parse server version")]
pub enum ParseServerVersionError {
    #[error("{0}")]
    ParseIntError(#[from] ParseIntError),
    #[error("invalid version pattern")]
    InvalidPattern,
}

#[derive(Debug, Clone, Default, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ServerType {
    #[default]
    NvidiaGameStream,
    Sunshine,
    Apollo,
}

/// The version of the server.
///
/// This is the [app_version field](crate::http::server_info::ServerInfoResponse::app_version) in the [ServerInfoResponse](crate::http::server_info::ServerInfoResponse).
#[derive(Debug, Clone, Copy)]
pub struct ServerVersion {
    pub major: i32,
    pub minor: i32,
    pub patch: i32,
    pub mini_patch: i32,
    /// This type is more a guess, but gives a hint into the version
    pub server_type: ServerType,
}

impl ServerVersion {
    pub const fn new(major: i32, minor: i32, patch: i32, mini_patch: i32) -> ServerVersion {
        let mut server_type = ServerType::NvidiaGameStream;
        if mini_patch < 0 {
            server_type = ServerType::Sunshine;
        }

        Self {
            major,
            minor,
            patch,
            mini_patch,
            server_type,
        }
    }

    /// If the server software is the old nvidia server
    pub fn is_nvidia_software(&self) -> bool {
        matches!(self.server_type, ServerType::NvidiaGameStream)
    }

    /// This includes Sunshine, Apollo, Wolf and likely any other version that isn't Nvidia's Gamestream and supports newer protocols
    pub fn is_sunshine_like(&self) -> bool {
        // https://github.com/moonlight-stream/moonlight-common-c/blob/b126e481a195fdc7152d211def17190e3434bcce/src/Limelight-internal.h#L85
        self.mini_patch < 0 || matches!(self.server_type, ServerType::Sunshine | ServerType::Apollo)
    }

    pub fn is_apollo(&self) -> bool {
        matches!(self.server_type, ServerType::Apollo)
    }
}

impl PartialEq for ServerVersion {
    fn eq(&self, other: &Self) -> bool {
        self.major == other.major
            && self.minor == other.minor
            && self.patch == other.patch
            && self.mini_patch == other.mini_patch
    }
}
impl Eq for ServerVersion {}

impl Ord for ServerVersion {
    fn cmp(&self, other: &Self) -> Ordering {
        match self.major.cmp(&other.major) {
            Ordering::Equal => {}
            ordering => return ordering,
        }

        match self.minor.cmp(&other.minor) {
            Ordering::Equal => {}
            ordering => return ordering,
        }

        self.patch.cmp(&other.patch)
    }
}
impl PartialOrd for ServerVersion {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Display for ServerVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}.{}.{}.{}",
            self.major, self.minor, self.patch, self.mini_patch
        )
    }
}

impl FromStr for ServerVersion {
    type Err = ParseServerVersionError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut split = s.splitn(4, ".");

        let major = split
            .next()
            .ok_or(ParseServerVersionError::InvalidPattern)?
            .parse()?;
        let minor = split
            .next()
            .ok_or(ParseServerVersionError::InvalidPattern)?
            .parse()?;
        let patch = split
            .next()
            .ok_or(ParseServerVersionError::InvalidPattern)?
            .parse()?;
        let mini_patch = split
            .next()
            .ok_or(ParseServerVersionError::InvalidPattern)?
            .parse()?;

        Ok(Self::new(major, minor, patch, mini_patch))
    }
}
