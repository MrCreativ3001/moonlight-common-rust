//!
//! This module contains different crypto implementations
//!

#[cfg(feature = "openssl")]
pub mod openssl;

#[cfg(feature = "rustcrypto")]
pub mod rustcrypto;
