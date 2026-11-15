use std::{
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    sync::Arc,
};

use sans_io_time::Instant;

use crate::{
    ServerVersion,
    crypto::disabled::DisabledCryptoBackend,
    stream::{
        proto::{
            ping::{PingSender, PingSenderConfig},
            runtime::UdpStream,
            soonest,
            video::{VideoStream, VideoStreamConfig, depayloader::VideoDepayloaderConfig},
        },
        video::VideoFormat,
    },
};

const ADDR: SocketAddr = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(192, 168, 178, 1), 47998));

fn sunshine_gen_7_431() -> ServerVersion {
    ServerVersion::new(7, 1, 431, -1)
}

#[test]
fn ping_behavior_before_receiving_data() {
    // While no data is received the audio stream should mimic the ping sender
    let mut now = Instant::ZERO;

    let mut stream = VideoStream::new(
        now,
        VideoStreamConfig {
            addr: ADDR,
            queue: VideoDepayloaderConfig {
                packet_size: 1024,
                format: VideoFormat::Av1Main8,
                server_version: sunshine_gen_7_431(),
                encryption: None,
            },
            fps: 60,
            sunshine_ping: None,
        },
        Arc::new(DisabledCryptoBackend),
    );

    let mut sender = PingSender::new(
        now,
        PingSenderConfig {
            sunshine_ping: None,
        },
    );

    for i in 0..100 {
        let timeout = sender.poll_timeout();
        let stream_timeout = stream.poll_timeout();

        if soonest(timeout, stream_timeout) == stream_timeout {
            continue;
        }
        assert_eq!(timeout, stream_timeout, "iteration: {i}, now: {now}");
        let timeout = timeout.unwrap();
        assert!(timeout > now);

        assert_eq!(
            sender.poll_transmit(),
            stream.poll_transmit().map(|x| x.data)
        );

        now = timeout;
        sender.handle_timeout(now);
        stream.handle_timeout(now).unwrap();
    }
}
