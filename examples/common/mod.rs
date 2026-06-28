#![allow(clippy::unwrap_used)]
#![allow(unused)]

use std::{fs, net::IpAddr, path::Path, str::FromStr};

use clap::Parser;
use moonlight_common::http::{
    ClientIdentifier, ClientSecret, DEFAULT_HTTP_PORT, DEFAULT_UNIQUE_ID, ServerIdentifier,
};
use pem::Pem;
use tokio::task::spawn_blocking;
use tracing::Level;
use tracing_subscriber::{
    EnvFilter, filter::Directive, fmt, layer::SubscriberExt, util::SubscriberInitExt,
};
use venator::Venator;

pub mod gstreamer_audio;
pub mod gstreamer_mic;
pub mod gstreamer_video;

#[derive(Parser, Debug)]
#[command(version)]
pub struct Args {
    /// The address of the host
    pub address: IpAddr,
    /// The http port of the host
    #[arg(short, long, default_value_t = DEFAULT_HTTP_PORT)]
    pub port: u16,
    #[arg(short, long, default_value_t = DEFAULT_UNIQUE_ID.to_owned())]
    pub unique_id: String,
}

pub const EXAMPLE_DATA_DIR: &str = "./example-data";
pub const KEY_FILE: &str = "key.pem";
pub const CERTIFICATE_FILE: &str = "certificate.pem";
pub const SERVER_CERTIFICATE_FILE: &str = "server_certificate.pem";

pub fn init() {
    // Init tracing
    let audio_directive: Directive = "moonlight_common::stream::proto::audio::depayloader=debug"
        .parse()
        .unwrap();
    let video_directive: Directive = "moonlight_common::stream::proto::video::depayloader=debug"
        .parse()
        .unwrap();
    let control_directive: Directive = "moonlight_common::stream::proto::control=debug"
        .parse()
        .unwrap();
    let std: Directive = "moonlight_common::stream::std=debug".parse().unwrap();
    let tokio: Directive = "moonlight_common::stream::tokio=debug".parse().unwrap();

    let venator = Venator::default();

    // TODO: make this use the default level by default
    tracing_subscriber::registry()
        .with(venator)
        .with(fmt::layer())
        .with(
            EnvFilter::builder()
                .with_default_directive(Level::TRACE.into())
                .from_env_lossy()
                .add_directive(audio_directive)
                .add_directive(video_directive)
                .add_directive(control_directive)
                .add_directive(std)
                .add_directive(tokio),
        )
        .init();
}

pub fn try_load_identity(
    prefix: &str,
) -> Option<(ClientIdentifier, ClientSecret, ServerIdentifier)> {
    let key_path = format!("{}/{}_{}", EXAMPLE_DATA_DIR, prefix, KEY_FILE);
    let certificate_path = format!("{}/{}_{}", EXAMPLE_DATA_DIR, prefix, CERTIFICATE_FILE);
    let server_certificate_path = format!(
        "{}/{}_{}",
        EXAMPLE_DATA_DIR, prefix, SERVER_CERTIFICATE_FILE
    );

    if Path::new(&key_path).exists()
        && Path::new(&certificate_path).exists()
        && Path::new(&server_certificate_path).exists()
    {
        let certificate = fs::read_to_string(&certificate_path).unwrap();
        let key = fs::read_to_string(&key_path).unwrap();
        let server_certificate = fs::read_to_string(&server_certificate_path).unwrap();

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
    prefix: &str,
    client_identifier: &ClientIdentifier,
    client_secret: &ClientSecret,
    server_identifier: &ServerIdentifier,
) {
    let key_path = format!("{}/{}_{}", EXAMPLE_DATA_DIR, prefix, KEY_FILE);
    let certificate_path = format!("{}/{}_{}", EXAMPLE_DATA_DIR, prefix, CERTIFICATE_FILE);
    let server_certificate_path = format!(
        "{}/{}_{}",
        EXAMPLE_DATA_DIR, prefix, SERVER_CERTIFICATE_FILE
    );

    let certificate = client_identifier.to_pem().to_string();
    let key = client_secret.to_pem().to_string();
    let server_certificate = server_identifier.to_pem().to_string();

    fs::create_dir_all(EXAMPLE_DATA_DIR).unwrap();

    fs::write(certificate_path, certificate).unwrap();
    fs::write(key_path, key).unwrap();
    fs::write(server_certificate_path, server_certificate).unwrap();
}

pub async fn try_load_identity_async(
    prefix: &str,
) -> Option<(ClientIdentifier, ClientSecret, ServerIdentifier)> {
    let prefix = prefix.to_string();
    spawn_blocking(move || try_load_identity(&prefix))
        .await
        .unwrap()
}

pub async fn save_identity_async(
    prefix: &str,
    client_identifier: &ClientIdentifier,
    client_secret: &ClientSecret,
    server_identifier: &ServerIdentifier,
) {
    let client_identifier = client_identifier.clone();
    let client_secret = client_secret.clone();
    let server_identifier = server_identifier.clone();

    let prefix = prefix.to_string();

    spawn_blocking(move || {
        save_identity(
            &prefix,
            &client_identifier,
            &client_secret,
            &server_identifier,
        );
    })
    .await
    .unwrap();
}
