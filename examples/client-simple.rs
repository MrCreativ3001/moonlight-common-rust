#![allow(clippy::unwrap_used)]

use std::{fs, sync::Arc};

use moonlight_common::{
    crypto::rustcrypto::RustCryptoBackend,
    high::std::MoonlightHost,
    http::{
        DEFAULT_HTTP_PORT, DEFAULT_UNIQUE_ID,
        client::ureq::UreqClient,
        pair::{PairPin, PairingCryptoBackend},
    },
};
use tracing::info;

use crate::common::{EXAMPLE_DATA_DIR, save_identity, try_load_identity};

mod common;

fn main() {
    common::init();

    let address = "192.168.178.140".to_string();
    // let address = "localhost".to_string();

    let http_port = DEFAULT_HTTP_PORT;
    let unique_id = DEFAULT_UNIQUE_ID.to_string();

    // Create a new client that'll use the [UreqClient] in the background to make requests
    let client =
        MoonlightHost::<UreqClient>::new(address.clone(), http_port, Some(unique_id)).unwrap();

    // Create a Crypto Backend
    let crypto_provider = Arc::new(RustCryptoBackend);

    // -- Pair to a host

    // Try to get existing identity
    match try_load_identity() {
        Some((client_identifier, client_secret, server_identifier)) => {
            // Set already existing identity
            client
                .set_identity(client_identifier, client_secret, server_identifier)
                .unwrap();
        }
        None => {
            // Generate new identity
            let (client_identifier, client_secret) =
                crypto_provider.generate_client_identity().unwrap();

            // Pair to sunshine server and print a message
            let device_name = "roth".to_string();
            let pin = PairPin::new(1, 2, 3, 4).unwrap();

            info!("Enter the pin {pin} for the host \"{address}\" to pair.");

            client
                .pair(
                    &client_identifier,
                    &client_secret,
                    device_name,
                    pin,
                    crypto_provider.clone(),
                )
                .unwrap();

            let (_, _, server_identifier) = client.identity().unwrap();

            // Save identity and server identifier
            save_identity(&client_identifier, &client_secret, &server_identifier);
        }
    };

    // -- Save all app images to the client directory by app name

    // Create folder for the app images
    fs::create_dir_all(format!("{EXAMPLE_DATA_DIR}/apps/")).unwrap();

    // Get all apps the host has
    let apps = client.app_list().unwrap();

    // Iterate through those apps
    for app in apps {
        // Request the app image from the host (you should cache them somehow)
        let app_image_bytes = client.request_app_image(app.id).unwrap();

        // Write the image to a file
        fs::write(
            format!("{EXAMPLE_DATA_DIR}/apps/{}.png", app.title),
            app_image_bytes,
        )
        .unwrap();
    }
}
