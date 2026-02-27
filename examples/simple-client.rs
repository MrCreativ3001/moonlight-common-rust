use std::sync::Arc;

use moonlight_common::{
    crypto::openssl::OpenSSLCryptoBackend,
    high::std::MoonlightHost,
    http::{DEFAULT_HTTP_PORT, DEFAULT_UNIQUE_ID, client::reqwest_blocking::ReqwestClient},
};

mod common;

fn main() {
    common::init();

    // Create a new client that'll use the [reqwest::Client] in the background to make requests
    // let address = "192.168.178.140".to_string();
    let address = "localhost".to_string();
    let http_port = DEFAULT_HTTP_PORT;
    let unique_id = DEFAULT_UNIQUE_ID.to_string();

    let client =
        MoonlightHost::<ReqwestClient>::new(address.clone(), http_port, Some(unique_id)).unwrap();

    // Create a Crypto Backend
    let crypto_provider = Arc::new(OpenSSLCryptoBackend::default());

    // Try to get existing identity
    match try_load_identity() {
        Some((client_identifier, client_secret, server_identifier)) => {
            // Set already existing identity identity
            client
                .set_identity(&client_identifier, &client_secret, &server_identifier)
                .unwrap();
        }
        None => {
            // Pair using new identity
            let (client_identifier, client_secret) =
                crypto_provider.generate_client_identity().unwrap();

            // Pair to sunshine server and print a message
            let device_name = "roth".to_string();
            let pin = PairPin::new(1, 2, 3, 4).unwrap();

            println!("Enter the pin {pin} for the host \"{address}\" to pair.");

            client
                .pair(
                    &client_identifier,
                    &client_secret,
                    device_name,
                    pin,
                    // TODO: replace with rustcrypto
                    crypto_provider.clone(),
                )
                .unwrap();

            let (_, _, server_identifier) = client.identity().unwrap();

            // Save identity and server identifier
            save_identity(&client_identifier, &client_secret, &server_identifier);
        }
    };

    // TODO: start stream using tokio
}
