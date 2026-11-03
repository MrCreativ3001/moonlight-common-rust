use std::{
    net::{Ipv4Addr, SocketAddr},
    sync::Arc,
    time::Duration,
};

use bytes::Bytes;
use sans_io_time::Instant;

use crate::{
    ServerVersion,
    crypto::rustcrypto::RustCryptoBackend,
    stream::{
        AesKey,
        proto::{
            runtime::{Receive, UdpStream},
            video::{
                VideoStream, VideoStreamConfig, VideoStreamEvent,
                depayloader::VideoDepayloaderConfig,
                packet::{
                    EncryptedVideoHeader, RtpVideoHeader, VIDEO_RECV_BUFFERED_PACKETS, VideoHeader,
                },
                payloader::{VideoPayloader, VideoPayloaderConfig},
            },
        },
        video::{FrameIndex, ServerFrameType, VideoFormat},
    },
};

const KEY: AesKey = AesKey([0x42; 16]);
const PACKET_SIZE: usize = 48 + VideoHeader::SIZE;

fn server_version() -> ServerVersion {
    ServerVersion::new(7, 1, 431, -1)
}

fn addr() -> SocketAddr {
    SocketAddr::new(Ipv4Addr::LOCALHOST.into(), 47998)
}

fn at(seconds: u64) -> Instant {
    Instant::from_nanos(0) + Duration::from_secs(seconds)
}

fn stream(key: Option<AesKey>) -> VideoStream {
    VideoStream::new(
        at(0),
        VideoStreamConfig {
            addr: addr(),
            queue: VideoDepayloaderConfig {
                packet_size: PACKET_SIZE,
                format: VideoFormat::Av1Main8,
                server_version: server_version(),
            },
            fps: 60,
            sunshine_ping: None,
            sunshine_encryption: key,
        },
        Arc::new(RustCryptoBackend),
    )
}

/// One packet list per frame
fn frames(key: AesKey, count: usize) -> Vec<Vec<Bytes>> {
    let mut payloader = VideoPayloader::new_encrypted(
        VideoPayloaderConfig {
            server_version: server_version(),
            packet_size: PACKET_SIZE,
            fec: None,
            max_data_shards_per_block: 100,
        },
        key,
        Arc::new(RustCryptoBackend),
    );

    (0..count)
        .map(|index| {
            let frame_type = if index == 0 {
                ServerFrameType::Idr
            } else {
                ServerFrameType::PFrame
            };
            payloader
                .push_frame(0, None, frame_type, &[7; 100])
                .expect("test frame should be packetized");

            std::iter::from_fn(|| payloader.poll_packet()).collect()
        })
        .collect()
}

fn receive(stream: &mut VideoStream, now: Instant, data: &[u8]) {
    stream
        .handle_receive(
            now,
            Receive {
                source: addr(),
                data,
            },
        )
        .expect("a received video packet must not end the stream");
}

fn delivered_frames(stream: &mut VideoStream) -> Vec<FrameIndex> {
    std::iter::from_fn(|| stream.poll_event())
        .filter_map(|event| match event {
            VideoStreamEvent::OnFrame(frame) => Some(frame.metadata.frame_index),
            VideoStreamEvent::SignalIdr => None,
        })
        .collect()
}

#[test]
fn delivers_authenticated_frames() {
    let mut stream = stream(Some(KEY));

    for packets in frames(KEY, 2) {
        for packet in packets {
            receive(&mut stream, at(0), &packet);
        }
    }

    assert_eq!(
        delivered_frames(&mut stream),
        [FrameIndex(1), FrameIndex(2)]
    );
}

#[test]
fn spoofed_datagrams_cant_end_the_stream() {
    let mut stream = stream(Some(KEY));
    let frames = frames(KEY, 1);

    let mut tampered = frames[0][0].to_vec();
    tampered[40] ^= 1;

    for datagram in [&[][..], &[1, 2, 3], &[0; 96], &[0; 4000], &tampered] {
        receive(&mut stream, at(0), datagram);
    }
    assert_eq!(delivered_frames(&mut stream), []);

    for packet in &frames[0] {
        receive(&mut stream, at(0), packet);
    }
    assert_eq!(delivered_frames(&mut stream), [FrameIndex(1)]);
}

#[test]
fn replayed_frames_arent_delivered_again() {
    let mut stream = stream(Some(KEY));
    let frames = frames(KEY, 2);

    for packet in frames.iter().flatten() {
        receive(&mut stream, at(0), packet);
    }
    assert_eq!(
        delivered_frames(&mut stream),
        [FrameIndex(1), FrameIndex(2)]
    );

    for packet in frames.iter().flatten() {
        receive(&mut stream, at(0), packet);
    }
    assert_eq!(delivered_frames(&mut stream), []);

    let stats = stream.depayloader.decryption_stats().unwrap();
    assert_eq!(stats.dropped_stale, frames.iter().flatten().count() as u64,);
}

#[test]
fn wrong_key_fails_the_stream_instead_of_freezing() {
    let mut stream = stream(Some(KEY));
    let frames = frames(AesKey([0x43; 16]), 1);

    receive(&mut stream, at(0), &frames[0][0]);
    stream
        .handle_timeout(at(9))
        .expect("still within the timeout");

    assert!(stream.handle_timeout(at(10)).is_err());
    assert_eq!(delivered_frames(&mut stream), []);
}

#[test]
fn spoofed_datagrams_after_authentication_dont_fail_the_stream() {
    let mut stream = stream(Some(KEY));
    let frames = frames(KEY, 1);

    for packet in &frames[0] {
        receive(&mut stream, at(0), packet);
    }

    receive(&mut stream, at(30), &[0; 96]);
    stream.handle_timeout(at(60)).unwrap();
}

#[test]
fn unencrypted_streams_have_no_authentication_timeout() {
    let mut stream = stream(None);

    stream.handle_timeout(at(60)).unwrap();
}

#[test]
fn receive_buffer_fits_the_encryption_header() {
    let plaintext = RtpVideoHeader::SIZE + PACKET_SIZE;

    assert_eq!(
        stream(None).recv_buffer_hint(),
        Some(VIDEO_RECV_BUFFERED_PACKETS * plaintext)
    );
    assert_eq!(
        stream(Some(KEY)).recv_buffer_hint(),
        Some(VIDEO_RECV_BUFFERED_PACKETS * (EncryptedVideoHeader::SIZE + plaintext))
    );
}
