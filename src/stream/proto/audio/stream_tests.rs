use std::{
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    sync::Arc,
    time::Duration,
};

use sans_io_time::Instant;

use crate::{
    crypto::disabled::DisabledCryptoBackend,
    stream::{
        audio::OpusMultistreamConfig,
        proto::{
            audio::{
                AudioStream, AudioStreamConfig,
                payloader::{AudioPayloader, AudioPayloaderConfig},
            },
            runtime::{Receive, UdpStream},
        },
    },
};

const ADDR: SocketAddr = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(192, 168, 178, 1), 48000));

#[test]
fn no_busy_loop_when_not_receiving_data() {
    let mut now = Instant::ZERO;
    let mut audio_stream = AudioStream::new_unencrypted(
        now,
        AudioStreamConfig {
            addr: ADDR,
            opus_config: OpusMultistreamConfig::STEREO,
            audio_packet_duration: Duration::from_millis(5),
            fec: false,
            sunshine_encryption: None,
            sunshine_ping: None,
        },
    );

    loop {
        now += Duration::from_millis(1);

        let timeout = audio_stream.poll_timeout();
        if timeout.is_some_and(|timeout| timeout < now) || timeout.is_none() {
            panic!("audio stream shouldn't have anything to do");
        }

        while audio_stream.poll_event().is_some() {}
        while audio_stream.poll_transmit().is_some() {}

        audio_stream.handle_timeout(now).unwrap();
    }
}

#[test]

fn no_busy_loop_after_skipping_data() {
    let mut now = Instant::ZERO;
    let mut audio_stream = AudioStream::new_unencrypted(
        now,
        AudioStreamConfig {
            addr: ADDR,
            opus_config: OpusMultistreamConfig::STEREO,
            audio_packet_duration: Duration::from_millis(5),
            fec: false,
            sunshine_encryption: None,
            sunshine_ping: None,
        },
    );

    let mut payloader = AudioPayloader::new(
        AudioPayloaderConfig {
            encryption: None,
            fec: false,
            frame_len: 4,
        },
        Arc::new(DisabledCryptoBackend),
    );
    payloader
        .push_frame(Duration::from_millis(0), &[0, 1, 2, 3])
        .unwrap();
    payloader
        .push_frame(Duration::from_millis(5), &[4, 5, 6, 7])
        .unwrap();

    audio_stream
        .handle_receive(
            now,
            Receive {
                source: ADDR,
                data: &payloader.poll_packet().unwrap(),
            },
        )
        .unwrap();
    audio_stream
        .handle_receive(
            now,
            Receive {
                source: ADDR,
                data: &payloader.poll_packet().unwrap(),
            },
        )
        .unwrap();

    loop {
        now += Duration::from_millis(1);

        let timeout = audio_stream.poll_timeout();
        if timeout.is_some_and(|timeout| timeout < now) || timeout.is_none() {
            panic!("audio stream shouldn't have anything to do");
        }

        while audio_stream.poll_event().is_some() {}
        while audio_stream.poll_transmit().is_some() {}

        audio_stream.handle_timeout(now).unwrap();
    }
}
