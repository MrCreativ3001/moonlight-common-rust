//!
//! This module contains different crypto implementations
//!

pub mod disabled;

#[cfg(feature = "openssl")]
pub mod openssl;

#[cfg(feature = "rustcrypto")]
pub mod rustcrypto;
