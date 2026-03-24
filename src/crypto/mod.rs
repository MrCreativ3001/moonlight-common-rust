//!
//! This module contains different crypto implementations
//!

pub mod disabled;

#[cfg(feature = "openssl")]
pub mod openssl;

#[cfg(feature = "rustcrypto")]
pub mod rustcrypto;

/// References:
/// - https://github.com/moonlight-stream/moonlight-common-c/blob/62687809b1f7410c3db4be2527503a54ae408d70/src/PlatformCrypto.h#L22
pub const fn round_to_pkcs7_padded_len(x: usize) -> usize {
    ((x + 15) / 16) * 16
}
