use std::net::SocketAddr;

use uniffi::{
    custom_type,
    deps::anyhow::{Error, anyhow},
    remote, setup_scaffolding,
};

use moonlight_common::stream::{
    AesIv, AesKey, SunshineEncryption,
    proto::{Instant, packet::SunshinePing},
};

pub mod audio_stream;
pub mod log;

setup_scaffolding!();

custom_type!(Instant, i64, {
    remote,
    // Lowering the Rust Instant into a u64.
    lower: |instant| instant.as_nanos(),
    // Lifting the foreign u64 into the Rust Instant
    try_lift: |nanos| Result::<_, Error>::Ok(Instant::from_nanos(nanos)),
});

custom_type!(SocketAddr, String, {
    remote,
    lower: |addr| addr.to_string(),
    try_lift: |text| Ok(text.parse()?),
});

custom_type!(AesKey, Vec<u8>, {
    remote,
    lower: |key| key.0.to_vec(),
    try_lift: |vec| Ok(AesKey(vec.as_array::<16>().copied().ok_or_else(|| anyhow!("The length of the AesKey must be 16 bytes! (current: {})", vec.len()))?)),
});

custom_type!(AesIv, u32, {
    remote,
    lower: |iv| iv.0,
    try_lift: |num| Ok(AesIv(num)),
});

#[remote(Record)]
pub struct SunshineEncryption {
    pub aes_key: AesKey,
    pub aes_iv: AesIv,
}

custom_type!(SunshinePing, Vec<u8>, {
    remote,
    lower: |ping| ping.0.to_vec(),
    try_lift: |vec| Ok(SunshinePing(vec.as_array::<16>().copied().ok_or_else(|| anyhow!("The length of the SunshinePing must be 16 bytes! (current: {})", vec.len()))?)),
});
