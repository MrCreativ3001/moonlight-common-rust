#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use moonlight_common::{
    crypto::openssl::OpenSSLCryptoBackend,
    high::tokio::MoonlightHost,
    http::{
        ClientIdentifier, ClientSecret, DEFAULT_HTTP_PORT, DEFAULT_UNIQUE_ID, ServerIdentifier,
        client::reqwest::ReqwestClient,
        pair::{PairPin, PairingCryptoBackend},
    },
};

#[tokio::main]
async fn main() {
    // Create a new client that'll use the [reqwest::Client] in the background to make requests
    let address = "localhost".to_string();
    let http_port = DEFAULT_HTTP_PORT;
    let unique_id = DEFAULT_UNIQUE_ID.to_string();

    let client =
        MoonlightHost::<ReqwestClient>::new(address.clone(), http_port, Some(unique_id)).unwrap();

    // Create a Crypto Backend
    let crypto_provider = Arc::new(OpenSSLCryptoBackend);

    // Generate new identity
    let (identifier, secret) = crypto_provider.generate_client_identity().unwrap();

    // Pair to sunshine server and print a message
    let device_name = "roth".to_string();
    let pin = PairPin::new(1, 2, 3, 4).unwrap();

    println!("Enter the pin {pin} for the host \"{address}\" to pair.");

    client
        .pair(
            &identifier,
            &secret,
            device_name,
            pin,
            // TODO: replace with rustcrypto
            crypto_provider.clone(),
        )
        .await
        .unwrap();

    // Save identity and server identifier
    // TODO
}

async fn save_identity(
    client_identifier: &ClientIdentifier,
    client_secret: &ClientSecret,
    server_identifier: &ServerIdentifier,
) {
    todo!()
}
