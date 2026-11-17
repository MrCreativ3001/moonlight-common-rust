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
            runtime::{Receive, Transmit, UdpStream, drive_sleep, drive_until},
            video::{
                depayloader::VideoDepayloaderConfig,
                payloader::{VideoPayloader, VideoPayloaderConfig},
                stream::{
                    FULL_FRAME_RECEIVE_TIMEOUT, VideoStream, VideoStreamConfig, VideoStreamEvent,
                },
            },
        },
        video::{FrameIndex, ServerFrameType, VideoFormat},
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

    let mut stream_now = now;
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

    for _ in 0..10 {
        now = sender.poll_timeout().unwrap();
        let transmits = std::iter::from_fn(|| {
            sender.poll_transmit().map(|data| Transmit {
                destination: ADDR,
                data,
            })
        })
        .collect::<Vec<_>>();

        let _ = drive_until(&mut stream, now, &mut stream_now, &transmits);

        sender.handle_timeout(now);
    }
}

#[test]
fn request_idr_on_loss_and_recover() {
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

    let mut payloader = VideoPayloader::new(
        VideoPayloaderConfig {
            server_version: sunshine_gen_7_431(),
            packet_size: 1024,
            max_data_shards_per_block: 100,
            fec: None,
            encryption: None,
        },
        Arc::new(DisabledCryptoBackend),
    );

    // send the first packet
    payloader
        .push_frame(0, None, ServerFrameType::Idr, &[0, 1, 2, 3, 4])
        .unwrap();
    stream
        .handle_receive(
            now,
            Receive {
                source: ADDR,
                data: &payloader.poll_packet().unwrap(),
            },
        )
        .unwrap();
    assert_eq!(payloader.poll_packet(), None);

    // Simulate dropping packets
    for _ in 0..3 {
        payloader
            .push_frame(0, None, ServerFrameType::PFrame, &[0, 1, 2, 3, 4])
            .unwrap();
        assert!(payloader.poll_packet().is_some());
        assert_eq!(payloader.poll_packet(), None);
    }

    // Wait until an idr request
    let events = drive_sleep(&mut stream, &mut now, FULL_FRAME_RECEIVE_TIMEOUT, &[]);
    assert!(
        events
            .iter()
            .any(|event| matches!(event, VideoStreamEvent::SignalIdr)),
        "no idr request: {events:?}"
    );

    // Insert the next Idr and test for receive
    payloader
        .push_frame(0, None, ServerFrameType::Idr, &[0, 1, 2, 3, 4])
        .unwrap();
    stream
        .handle_receive(
            now,
            Receive {
                source: ADDR,
                data: &payloader.poll_packet().unwrap(),
            },
        )
        .unwrap();
    assert_eq!(payloader.poll_packet(), None);

    let events = drive_until(&mut stream, now, &mut now, &[]);
    assert!(
        events
            .iter()
            .any(|event| matches!(event, VideoStreamEvent::OnFrame(frame) if frame.metadata().frame_index == FrameIndex(5))),
        "no frame: {events:?}"
    );
}
