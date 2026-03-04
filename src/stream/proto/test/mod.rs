use std::{
    net::{IpAddr, Ipv4Addr},
    time::Instant,
};

use crate::{
    ServerVersion,
    stream::{
        AesIv, AesKey, MoonlightStreamConfig,
        proto::{MoonlightStreamAction, MoonlightStreamOutput},
        video::ServerCodecModeSupport,
    },
};

fn assert_eq_output(value: MoonlightStreamOutput, expected: MoonlightStreamOutput) {
    let matches = match &value {
        MoonlightStreamOutput::Timeout(t1) => {
            if let MoonlightStreamOutput::Timeout(t2) = &expected {
                t1 == t2
            } else {
                false
            }
        }
        MoonlightStreamOutput::Action(MoonlightStreamAction::ConnectTcp { addr }) => {
            todo!()
        }
        MoonlightStreamOutput::Action(MoonlightStreamAction::SendTcp { data }) => {
            todo!()
        }
        MoonlightStreamOutput::Action(MoonlightStreamAction::SendUdp { to, data }) => {
            todo!()
        }
        MoonlightStreamOutput::Action(MoonlightStreamAction::StartAudioStream { .. })
            if matches!(
                expected,
                MoonlightStreamOutput::Action(MoonlightStreamAction::StartAudioStream { .. })
            ) =>
        {
            true
        }
        MoonlightStreamOutput::Action(MoonlightStreamAction::StartVideoStream { .. })
            if matches!(
                expected,
                MoonlightStreamOutput::Action(MoonlightStreamAction::StartVideoStream { .. })
            ) =>
        {
            true
        }
        MoonlightStreamOutput::Action(MoonlightStreamAction::StartControlStream { .. })
            if matches!(
                expected,
                MoonlightStreamOutput::Action(MoonlightStreamAction::StartControlStream { .. })
            ) =>
        {
            true
        }
        MoonlightStreamOutput::Event(e1) => {
            if let MoonlightStreamOutput::Event(e2) = &expected {
                e1 == e2
            } else {
                false
            }
        }
        _ => false,
    };

    assert!(
        matches,
        "Output doesn't matches:\nleft: {value:?}\nright: {expected:?}"
    );
}

#[test]
fn test_stream_start() {
    let addr = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));
    let rtsp_port = 48010;

    let config = MoonlightStreamConfig {
        address: addr.to_string(),
        version: ServerVersion::new(7, 1, 431, -1),
        rtsp_session_url: format!("rtsp://{addr}:{rtsp_port}"),
        gfe_version: "3.23.0.74".to_string(),
        server_codec_mode_support: ServerCodecModeSupport::H264,
        // Encryption is not used by this test
        remote_input_aes_key: AesKey([0; 16]),
        remote_input_aes_iv: AesIv(0),
        apollo_permissions: None,
    };

    let time = Instant::now();

    // TODO
    // let mut stream = MoonlightStreamProto::new(
    //     time,
    //     config,
    //     MoonlightStreamSettings {
    //         width: 1920,
    //         height: 1080,
    //         fps: 60,
    //         fps_x100: 60 * 100,
    //         bitrate: 10000,
    //         encryption_flags: EncryptionFlags::empty(),
    //         streaming_remotely: StreamingConfig::Local,
    //         audio_config: AudioConfig::STEREO,
    //         supported_video_formats: SupportedVideoFormats::H264,
    //         packet_size: 1024,
    //         color_range: ColorRange::Limited,
    //         color_space: ColorSpace::Rec709,
    //     },
    // )
    // .unwrap();

    // assert_eq_output(
    //     stream.poll_output().unwrap(),
    //     MoonlightStreamOutput::Action(MoonlightStreamAction::ConnectTcp {
    //         addr: SocketAddr::new(addr, rtsp_port),
    //     }),
    // );

    // stream
    //     .handle_input(MoonlightStreamInput::TcpConnect(time))
    //     .unwrap();

    // // TODO
    // assert_eq_output(stream.poll_output().unwrap(), todo!());
}

// TODO: remove test prefix from all tests
