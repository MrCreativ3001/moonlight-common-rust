#![allow(clippy::unwrap_used)]

use std::{io, sync::Arc, time::Duration};

use clap::Parser;
use moonlight_common::{
    crypto::rustcrypto::RustCryptoBackend,
    high::tokio::MoonlightHost,
    http::{
        client::tokio_hyper::TokioHyperClient,
        pair::{PairPin, PairingCryptoBackend},
    },
    stream::{
        AesIv, AesKey, EncryptionFlags, MoonlightStreamSettings, StreamingConfig,
        audio::{AudioConfig, AudioDecoder, AudioFrame, OpusMultistreamConfig},
        control::ActiveGamepads,
        proto::{
            MoonlightStreamSetup,
            audio::AudioStreamEvent,
            control::{ControlStreamEvent, input_batcher::ClientInputEvent, packet::ControlPacket},
            video::VideoStreamEvent,
        },
        tokio::{MoonlightStream, MoonlightStreamError},
        video::{
            ColorRange, ColorSpace, DecodeResult, VideoCapabilities, VideoDecodeUnit, VideoDecoder,
            VideoFormats, VideoSetup,
        },
    },
};
use tokio::{
    select, spawn,
    sync::{Mutex, mpsc},
    time::sleep,
};
use tracing::info;

use crate::common::{
    Args, gstreamer_audio::GStreamerAudioDecoder, gstreamer_video::GStreamerVideoDecoder,
    save_identity_async, try_load_identity_async,
};

mod common;

#[tokio::main]
async fn main() {
    common::init();

    let Args {
        address,
        port,
        unique_id,
    } = Args::parse();

    // Create a new client that'll use the [TokioHyperClient] in the background to make requests
    let client =
        MoonlightHost::<TokioHyperClient>::new(address.to_string(), port, Some(unique_id)).unwrap();

    // Create a Crypto Backend
    let crypto_backend = RustCryptoBackend;

    // Try to get existing identity
    match try_load_identity_async(&address.to_string()).await {
        Some((client_identifier, client_secret, server_identifier)) => {
            info!("Using existing identity");

            // Set already existing identity identity
            client
                .set_identity(client_identifier, client_secret, server_identifier)
                .await
                .unwrap();
        }
        None => {
            // Pair using new identity
            info!("Initializing pairing");

            // Generate new identity
            let (client_identifier, client_secret) =
                crypto_backend.generate_client_identity().unwrap();

            // Pair to sunshine server and print a message
            // This device name doesn't get used (i think), the default is "roth"
            let device_name = "roth".to_string();

            // Generate new pin
            let pin = PairPin::new_random(&crypto_backend).unwrap();

            info!("Enter the pin {pin} for the host \"{address}\" to pair.");

            client
                .pair(
                    &client_identifier,
                    &client_secret,
                    device_name,
                    pin,
                    crypto_backend.clone(),
                )
                .await
                .unwrap();

            let (_, _, server_identifier) = client.identity().await.unwrap();

            // Save identity and server identifier
            save_identity_async(
                &address.to_string(),
                &client_identifier,
                &client_secret,
                &server_identifier,
            )
            .await;

            info!("Successfully paired to host");
        }
    };

    // -- Start a stream

    // Get all apps
    let apps = client.app_list().await.unwrap();

    // Use the first app
    let app = &apps[0];

    // Set settings for the stream
    let mut settings = MoonlightStreamSettings {
        width: 1920,
        height: 1080,
        fps: 60,
        fps_x100: 60 * 100,
        bitrate: 2000,
        packet_size: 1024,
        encryption_flags: EncryptionFlags::all(),
        streaming_remotely: StreamingConfig::Auto,
        sops: true,
        hdr: false,
        supported_video_formats: VideoFormats::H264,
        color_space: ColorSpace::Rec709,
        color_range: ColorRange::Limited,
        local_audio_play_mode: false,
        audio_config: AudioConfig::STEREO,
        gamepads_attached: ActiveGamepads::empty(),
        gamepads_persist_after_disconnect: false,
        // Enable mic if the server supports it
        enable_mic: true,
    };

    // Adjust the settings for the host, required because older hosts might not support some settings
    // This can fail if the host doesn't support some configuration detail
    settings
        .adjust_for_server(
            client.version().await.unwrap(),
            &client.gfe_version().await.unwrap(),
            client.server_codec_mode_support().await.unwrap(),
        )
        .unwrap();

    // -- Create media pipelines
    gstreamer::init().unwrap();

    let mut audio_decoder = GStreamerAudioDecoder::new().unwrap();
    let mut video_decoder = GStreamerVideoDecoder::new().unwrap();

    // -- Start Stream
    // Generate an aes key and aes iv
    let aes_key = AesKey::new_random(&crypto_backend).unwrap();
    let aes_iv = AesIv::new_random(&crypto_backend).unwrap();

    // Initialize the starting phase on the server
    let config = client
        .start_stream(
            app.id,
            &settings,
            aes_key,
            aes_iv,
            MoonlightStreamSetup::launch_query_parameters(),
        )
        .await
        .unwrap();

    // Transition from the starting phase into the streaming phase
    let stream = Arc::new(
        MoonlightStream::connect(
            config,
            settings,
            Arc::new(crypto_backend),
            VideoCapabilities::default(),
        )
        .await
        .unwrap(),
    );

    // Setup and start the decoders
    // TODO: how to get the audio_config?
    audio_decoder.setup(AudioConfig::STEREO, stream.audio_setup());
    video_decoder.setup(stream.video_setup());

    // Audio
    spawn({
        let stream = stream.clone();
        async move {
            let mut started = false;

            while let Ok(frame) = stream.poll_audio_frame().await {
                if !started {
                    started = true;
                    audio_decoder.start();
                }

                audio_decoder.decode_and_play_sample(frame.as_ref());
            }

            audio_decoder.stop();
        }
    });

    // Video
    spawn({
        let stream = stream.clone();
        async move {
            loop {
                let mut started = false;

                while let Ok(frame) = stream.poll_video_frame().await {
                    if !started {
                        video_decoder.start();
                        started = true;
                    }

                    video_decoder.submit_decode_unit(frame.as_ref());
                }

                video_decoder.stop();
            }
        }
    });

    // Control
    spawn({
        let stream = stream.clone();
        async move {
            loop {
                while let Ok(packet) = stream.poll_packet().await {
                    info!(packet = ?packet, "receive control packet");
                }
            }
        }
    });

    // -- Use the stream

    // Move the cursor from the left side to the right side of the screen
    info!("starting mouse test");
    for i in 0..100 {
        // You should prefer to use send_mouse_move over send_mouse_position because it fails in multi monitor setups
        // See https://github.com/MrCreativ3001/moonlight-web-stream/issues/80
        // However this is just a simple example so we don't care
        stream
            .send_input(ClientInputEvent::MouseMoveAbsolute {
                x: i,
                y: 50,
                reference_width: 100,
                reference_height: 100,
            })
            .unwrap();

        sleep(Duration::from_secs(5) / 100).await;
    }
    info!("ending mouse test");

    // Wait a few seconds
    sleep(Duration::from_secs(100)).await;

    // Stop the stream
    stream.stop();
}
