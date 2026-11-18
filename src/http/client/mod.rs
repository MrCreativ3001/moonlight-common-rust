use ::std::time::Duration;

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
