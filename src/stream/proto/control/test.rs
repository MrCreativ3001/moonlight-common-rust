// TODO

use std::{
    error::Error,
    time::{Duration, Instant},
};

use crate::{
    crypto::disabled::DisabledCryptoBackend,
    stream::proto::{
        control::peer::{ControlHost, ControlHostConfig, ControlHostOutput},
        crypto::CryptoBackend,
    },
};

#[test]
fn client_server_peer() {
    let mut server = ControlHost::new(
        Instant::now(),
        ControlHostConfig {
            peer_count: 1,
            peer_channel_count: 1,
        },
        DisabledCryptoBackend,
    )
    .unwrap();

    let mut client = ControlHost::new(
        Instant::now(),
        ControlHostConfig {
            peer_count: 1,
            peer_channel_count: 1,
        },
        DisabledCryptoBackend,
    )
    .unwrap();

    // TODO: implement this
}
