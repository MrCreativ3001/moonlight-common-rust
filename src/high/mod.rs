//!
//! The high level api of this crate for easy usage.
//!

use ::std::{error::Error, sync::PoisonError};

use thiserror::Error;

use crate::{
    MoonlightError, high::tokio::StreamConfigError, http::pair::client::ClientPairingError,
};

#[derive(Debug, Error)]
pub enum MoonlightClientError {
    #[error("{0}")]
    Moonlight(#[from] MoonlightError),
    #[error("this action requires pairing")]
    NotPaired,
    #[error("{0}")]
    StreamConfig(#[from] StreamConfigError),
    #[error("the host is likely offline")]
    LikelyOffline,
    #[error("unauthenticated")]
    Unauthenticated,
    #[error("request: {0}")]
    Backend(Box<dyn Error + Send + Sync>),
    #[error("pairing: {0}")]
    Pairing(ClientPairingError<Box<dyn Error + Send + Sync>>),
    #[error("failed to make a request because the client is poisoned")]
    Poisoned(#[from] PoisonError<()>),
}

#[cfg(feature = "std")]
pub mod std;

#[cfg(feature = "tokio")]
pub mod tokio;
