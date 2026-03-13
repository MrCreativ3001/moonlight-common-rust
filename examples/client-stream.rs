#![allow(clippy::unwrap_used)]

use std::{sync::Arc, thread::sleep, time::Duration};

use moonlight_common::{
    crypto::rustcrypto::RustCryptoBackend,
    high::std::MoonlightHost,
    http::{DEFAULT_HTTP_PORT, DEFAULT_UNIQUE_ID, client::ureq::UreqClient},
    stream::{
        AesIv, AesKey, EncryptionFlags, MoonlightStreamSettings, StreamingConfig,
        audio::AudioConfig,
        control::ActiveGamepads,
        debug::{DebugListener, NullListener},
        std::MoonlightStream,
        video::{ColorRange, ColorSpace, SupportedVideoFormats},
    },
};

use crate::common::{
    gstreamer_audio::GStreamerAudioDecoder, gstreamer_video::GStreamerVideoDecoder,
    try_load_identity,
};

use tracing::info;

mod common;

fn main() {
    common::init();

    // This implementation is not done yet, use the client-common-c

    let address = "192.168.178.140".to_string();
    // let address = "localhost".to_string();

    let http_port = DEFAULT_HTTP_PORT;
    let unique_id = DEFAULT_UNIQUE_ID.to_string();

    // Create a new client that'll use the [UreqClient] in the background to make requests
    let client =
        MoonlightHost::<UreqClient>::new(address.clone(), http_port, Some(unique_id)).unwrap();

    // Create a Crypto Backend
    let crypto_backend = Arc::new(RustCryptoBackend);

    // -- Load identity
    let (client_identifier, client_secret, server_identifier) = try_load_identity().unwrap();

    client
        .set_identity(client_identifier, client_secret, server_identifier)
        .unwrap();

    // -- Start a stream

    // Get all apps
    let apps = client.app_list().unwrap();

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
        supported_video_formats: SupportedVideoFormats::H264,
        color_space: ColorSpace::Rec709,
        color_range: ColorRange::Limited,
        local_audio_play_mode: false,
        audio_config: AudioConfig::STEREO,
        gamepads_attached: ActiveGamepads::empty(),
        gamepads_persist_after_disconnect: false,
    };

    // Adjust the settings for the host, required because older hosts might not support some settings
    // This can fail if the host doesn't support some configuration detail
    settings
        .adjust_for_server(
            client.version().unwrap(),
            &client.gfe_version().unwrap(),
            client.server_codec_mode_support().unwrap(),
        )
        .unwrap();

    // -- Initialize Decoders

    // Initialize gstreamer
    gstreamer::init().unwrap();

    // Initialize Audio Decoder
    let audio_decoder = GStreamerAudioDecoder::new().unwrap();

    // Initialize Video Decoder
    let video_decoder = GStreamerVideoDecoder::new().unwrap();

    // -- Start Stream using the Decoders

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
            MoonlightStream::launch_query_parameters(),
        )
        .unwrap();

    // Transition from the starting phase into the streaming phase
    let stream =
        MoonlightStream::new(config, settings, NullListener, audio_decoder, DebugListener).unwrap();

    // TODO
    sleep(Duration::from_secs(1000));

    // Stop the stream: this will block
    // Dropping the [MoonlightStream] will also stop the stream without blocking the current thread
    stream.stop();

    info!("Stopped Stream");
}
