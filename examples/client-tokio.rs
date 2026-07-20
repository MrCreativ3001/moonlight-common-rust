#![allow(clippy::unwrap_used)]

use std::{pin::pin, sync::Arc, time::Duration};

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
        audio::{AudioConfig, AudioDecoder},
        control::ActiveGamepads,
        proto::{
            MoonlightStreamSetup,
            audio::AudioStreamEvent,
            control::{ControlStreamEvent, input_batcher::ClientInputEvent, packet::ControlPacket},
            video::VideoStreamEvent,
        },
        tokio::{MoonlightStream, MoonlightStreamEvent},
        video::{ColorRange, ColorSpace, VideoCapabilities, VideoDecoder, VideoFormats},
    },
};
use tokio::{
    select,
    time::{interval, sleep},
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
    let mut stream = MoonlightStream::connect(
        config,
        settings,
        Arc::new(crypto_backend),
        VideoCapabilities::default(),
    )
    .await
    .unwrap();

    // Setup the decoders
    // TODO: how to get the audio_config?
    audio_decoder.setup(AudioConfig::STEREO, stream.audio_setup());
    let mut audio_started = false;

    video_decoder.setup(stream.video_setup());
    let mut video_started = false;

    // Mouse Testing
    let mut i = 0;
    let mut interval = pin!(interval(Duration::from_secs(5) / 100));

    // Wait a few seconds for stop
    let mut stopped = false;
    let mut deadline = pin!(sleep(Duration::from_secs(20)));
    loop {
        if !stream.is_alive() {
            break;
        }

        select! {
            // Check for deadline
            _ = &mut deadline, if !stopped => {
                info!("stream deadline surpassed, stopping stream");
                stream.disconnect().unwrap();
                stopped = true;
            }
            // Do mouse test
            _ = interval.tick(), if (0..100).contains(&i) => {
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

                i += 1;
            }
            // Drive stream forward
            result = stream.drive() => {
                let event = result.unwrap();
                match event {
                    MoonlightStreamEvent::Audio(AudioStreamEvent::OnFrame(frame)) => {
                        if !audio_started {
                            audio_started = true;
                            audio_decoder.start();
                        }

                        audio_decoder.decode_and_play_sample(frame.as_ref());
                    }
                    MoonlightStreamEvent::Video(VideoStreamEvent::OnFrame(frame)) => {
                        if !video_started {
                            video_started = true;
                            video_decoder.start();
                        }

                        video_decoder.submit_decode_unit(frame.as_ref().into_decode_unit());
                    }
                    MoonlightStreamEvent::Video(VideoStreamEvent::SignalIdr) => {
                        let _ = stream.send_raw(ControlPacket::RequestIdr);
                    }
                    MoonlightStreamEvent::Control(ControlStreamEvent::Packet(packet)) => {
                        info!(packet = ?packet, "receive control packet");
                    }
                    MoonlightStreamEvent::Control(ControlStreamEvent::Disconnect) => {
                        info!("control stream disconnected");
                        break;
                    }
                    _ => {}
                }
            }
        }
    }

    audio_decoder.stop();
    video_decoder.stop();
}
