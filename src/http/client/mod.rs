use ::std::time::Duration;

use thiserror::Error;

pub mod async_client;
pub mod blocking_client;

#[cfg(feature = "std")]
pub mod std;

#[cfg(feature = "tokio")]
pub mod tokio;

#[cfg(any(feature = "std", feature = "tokio"))]
mod rustls;

pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);
pub const DEFAULT_LONG_TIMEOUT: Duration = Duration::from_secs(90);

#[cfg(any(feature = "std", feature = "tokio"))]
#[derive(Debug, Error)]
#[error("the authority is missing from the uri")]
struct AuthorityMissing;

#[cfg(any(feature = "std", feature = "tokio"))]
#[derive(Debug, Error)]
#[error("the given private key is not valid")]
struct InvalidPrivateKey;
