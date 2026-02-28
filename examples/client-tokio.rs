#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use moonlight_common::{
    crypto::openssl::OpenSSLCryptoBackend,
    high::tokio::MoonlightHost,
    http::{
        DEFAULT_HTTP_PORT, DEFAULT_UNIQUE_ID,
        client::hyper::HyperClient,
        pair::{PairPin, PairingCryptoBackend},
    },
};
use tracing::info;

use crate::common::{save_identity_async, try_load_identity_async};

mod common;

#[tokio::main]
async fn main() {
    common::init();

    // Create a new client that'll use the [reqwest::Client] in the background to make requests
    let address = "192.168.178.140".to_string();
    // let address = "localhost".to_string();

    let http_port = DEFAULT_HTTP_PORT;
    let unique_id = DEFAULT_UNIQUE_ID.to_string();

    let client =
        MoonlightHost::<HyperClient>::new(address.clone(), http_port, Some(unique_id)).unwrap();

    // Create a Crypto Backend
    let crypto_provider = Arc::new(OpenSSLCryptoBackend::default());

    // Try to get existing identity
    match try_load_identity_async().await {
        Some((client_identifier, client_secret, server_identifier)) => {
            info!("Using existing identity");

            // Set already existing identity identity
            client
                .set_identity(&client_identifier, &client_secret, &server_identifier)
                .await
                .unwrap();
        }
        None => {
            info!("Initializing pairing");

            // Pair using new identity
            let (client_identifier, client_secret) =
                crypto_provider.generate_client_identity().unwrap();

            // Pair to sunshine server and print a message
            // This device name doesn't get used (i think), the default is "roth"
            let device_name = "roth".to_string();

            // Generate new pin
            let pin = PairPin::new_random(&crypto_provider).unwrap();

            info!("Enter the pin {pin} for the host \"{address}\" to pair.");

            client
                .pair(
                    &client_identifier,
                    &client_secret,
                    device_name,
                    pin,
                    // TODO: replace with rustcrypto
                    crypto_provider.clone(),
                )
                .await
                .unwrap();

            let (_, _, server_identifier) = client.identity().await.unwrap();

            // Save identity and server identifier
            save_identity_async(&client_identifier, &client_secret, &server_identifier).await;

            info!("Successfully paired to host");
        }
    };

    // TODO: start stream using tokio
}
