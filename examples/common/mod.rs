#![allow(clippy::unwrap_used)]
#![allow(unused)]

use std::{fs, path::Path, str::FromStr};

use moonlight_common::http::{ClientIdentifier, ClientSecret, ServerIdentifier};
use pem::Pem;
use tokio::task::spawn_blocking;
use tracing::Level;
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

pub mod gstreamer_audio;
pub mod gstreamer_video;

pub const CLIENT_DIR: &str = "./client";
pub const KEY_PATH: &str = "./client/key.pem";
pub const CERTIFICATE_PATH: &str = "./client/certificate.pem";
pub const SERVER_CERTIFICATE_PATH: &str = "./client/server_certificate.pem";

pub fn init() {
    // Init tracing
    // TODO: make this use the default level by default
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(
            EnvFilter::builder()
                .with_default_directive(Level::DEBUG.into())
                .from_env_lossy(),
        )
        .init();
}

pub fn try_load_identity() -> Option<(ClientIdentifier, ClientSecret, ServerIdentifier)> {
    if Path::new(KEY_PATH).exists()
        && Path::new(CERTIFICATE_PATH).exists()
        && Path::new(SERVER_CERTIFICATE_PATH).exists()
    {
        let certificate = fs::read_to_string(CERTIFICATE_PATH).unwrap();
        let key = fs::read_to_string(KEY_PATH).unwrap();
        let server_certificate = fs::read_to_string(SERVER_CERTIFICATE_PATH).unwrap();

        Some((
            ClientIdentifier::from_pem(Pem::from_str(&certificate).unwrap()),
            ClientSecret::from_pem(Pem::from_str(&key).unwrap()),
            ServerIdentifier::from_pem(Pem::from_str(&server_certificate).unwrap()),
        ))
    } else {
        None
    }
}

pub fn save_identity(
    client_identifier: &ClientIdentifier,
    client_secret: &ClientSecret,
    server_identifier: &ServerIdentifier,
) {
    let certificate = client_identifier.to_pem().to_string();
    let key = client_secret.to_pem().to_string();
    let server_certificate = server_identifier.to_pem().to_string();

    fs::create_dir_all(CLIENT_DIR).unwrap();

    fs::write(CERTIFICATE_PATH, certificate).unwrap();
    fs::write(KEY_PATH, key).unwrap();
    fs::write(SERVER_CERTIFICATE_PATH, server_certificate).unwrap();
}

pub async fn try_load_identity_async() -> Option<(ClientIdentifier, ClientSecret, ServerIdentifier)>
{
    spawn_blocking(try_load_identity).await.unwrap()
}

pub async fn save_identity_async(
    client_identifier: &ClientIdentifier,
    client_secret: &ClientSecret,
    server_identifier: &ServerIdentifier,
) {
    let client_identifier = client_identifier.clone();
    let client_secret = client_secret.clone();
    let server_identifier = server_identifier.clone();

    spawn_blocking(move || {
        save_identity(&client_identifier, &client_secret, &server_identifier);
    })
    .await
    .unwrap();
}
