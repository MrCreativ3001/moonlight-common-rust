#![allow(clippy::unwrap_used)]

use std::{sync::Arc, thread::sleep, time::Duration};

use moonlight_common::{
    crypto::openssl::OpenSSLCryptoBackend,
    high::std::MoonlightHost,
    http::{DEFAULT_HTTP_PORT, DEFAULT_UNIQUE_ID, client::ureq::UreqClient},
    stream::{
        AesIv, AesKey, EncryptionFlags, MoonlightStreamSettings, StreamingConfig,
        audio::AudioConfig,
        c::MoonlightInstance,
        control::ActiveGamepads,
        debug::DebugListener,
        video::{ColorRange, ColorSpace, SupportedVideoFormats},
    },
};

use crate::common::{
    gstreamer_audio::GStreamerAudioDecoder, gstreamer_video::GStreamerVideoDecoder,
    try_load_identity,
};

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
    // Because we're using moonlight common c we can just use the OpenSSL backend, no need to include other crypto libraries
    let crypto_backend = Arc::new(OpenSSLCryptoBackend::default());

    // -- Load identity
    let Some((client_identifier, client_secret, server_identifier)) = try_load_identity() else {
        panic!("Please firstly use the pair example to pair to a host.");
    };

    client
        .set_identity(client_identifier, client_secret, server_identifier)
        .unwrap();

    // -- Start a stream

    // Get all apps
    let apps = client.app_list().unwrap();

    // Use the first app
    let app = &apps[0];

    // Get the moonlight common c library instance
    let instance = MoonlightInstance::global().unwrap();

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
    gstreamer::init().unwrap();
    let audio_decoder = GStreamerAudioDecoder::new().unwrap();
    let video_decoder = GStreamerVideoDecoder::new().unwrap();

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
            instance.launch_query_parameters(),
        )
        .unwrap();

    // Transition from the starting phase into the streaming phase
    let stream = instance
        .start_connection(
            config,
            settings,
            DebugListener,
            DebugListener,
            video_decoder,
            audio_decoder,
        )
        .unwrap();

    // Move the cursor from the left side to the right side of the screen
    for i in 0..100 {
        // You should prefer to use send_mouse_move over send_mouse_position because it fails in multi monitor setups
        // See https://github.com/MrCreativ3001/moonlight-web-stream/issues/80
        // However this is just a simple example so we don't care
        stream.send_mouse_position(i, 50, 100, 100).unwrap();

        sleep(Duration::from_secs(5) / 100);
    }

    // Wait some time to stop the stream
    sleep(Duration::from_secs(1000));

    // Stop the stream: this will block
    // Dropping the [MoonlightStream] will stop the stream
    stream.stop();
}
