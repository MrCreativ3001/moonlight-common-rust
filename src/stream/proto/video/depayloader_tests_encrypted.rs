use std::{sync::Arc, time::Duration};

use crate::{
    ServerVersion,
    crypto::rustcrypto::RustCryptoBackend,
    stream::{
        AesIv, AesKey, SunshineEncryption,
        proto::video::{
            depayloader::{VideoDepayloader, VideoDepayloaderConfig},
            frame::VideoFrame,
            packet::{RtpVideoHeader, VideoFrameHeader, VideoHeader, video_header_size},
            payloader::{VideoPayloader, VideoPayloaderConfig, VideoPayloaderFecConfig},
        },
        video::{
            BufferType, FrameIndex, FrameType, ServerFrameType, VideoFormat, VideoFrameBuffer,
        },
    },
};

fn sunshine_gen_7_431() -> ServerVersion {
    ServerVersion::new(7, 1, 431, -1)
}

#[test]
fn nofec_noparse() {
    let server_version = sunshine_gen_7_431();
    let encryption = SunshineEncryption {
        aes_key: AesKey([0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15]),
        aes_iv: AesIv(0),
    };
    let payload_size = 10;
    let packet_size = payload_size + video_header_size(true);
    let expected_host_processing_latency = Duration::from_millis(10);

    let expected_frame = [
        0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 9, 8, 7, 6, 5, 4, 3, 2, 1, 0, 0, 1,
    ];
    assert_eq!(expected_frame.len(), 30 - VideoFrameHeader::SIZE);

    let mut payloader = VideoPayloader::new(
        VideoPayloaderConfig {
            server_version,
            packet_size,
            fec: None,
            max_data_shards_per_block: 100,
            encryption: Some((encryption.aes_key, encryption.aes_iv)),
        },
        Arc::new(RustCryptoBackend),
    );
    payloader
        .push_frame(
            0,
            Some(expected_host_processing_latency),
            ServerFrameType::Idr,
            &expected_frame,
        )
        .unwrap();

    let mut depayloader = VideoDepayloader::new(
        VideoDepayloaderConfig {
            packet_size,
            // av1 doesn't get parsed
            format: VideoFormat::Av1Main8,
            server_version: sunshine_gen_7_431(),
            encryption: Some(encryption),
        },
        Arc::new(RustCryptoBackend),
    );

    // assert empty
    assert!(!depayloader.is_frame_known(FrameIndex(1)));
    assert!(!depayloader.is_frame_available(FrameIndex(1)));
    assert_eq!(depayloader.known_frames().collect::<Vec<_>>(), vec![]);
    assert_eq!(depayloader.available_frames().collect::<Vec<_>>(), vec![]);
    assert_eq!(depayloader.frame(FrameIndex(1)), None);

    // assert 1 packet
    depayloader
        .handle_packet(&payloader.poll_packet().unwrap())
        .unwrap();
    assert!(depayloader.is_frame_known(FrameIndex(1)));
    assert!(!depayloader.is_frame_available(FrameIndex(1)));
    assert_eq!(
        depayloader.known_frames().collect::<Vec<_>>(),
        vec![FrameIndex(1)]
    );
    assert_eq!(depayloader.available_frames().collect::<Vec<_>>(), vec![]);
    assert_eq!(depayloader.frame(FrameIndex(1)), None);

    // assert 2 packet
    depayloader
        .handle_packet(&payloader.poll_packet().unwrap())
        .unwrap();
    assert!(depayloader.is_frame_known(FrameIndex(1)));
    assert!(!depayloader.is_frame_available(FrameIndex(1)));
    assert_eq!(
        depayloader.known_frames().collect::<Vec<_>>(),
        vec![FrameIndex(1)]
    );
    assert_eq!(depayloader.available_frames().collect::<Vec<_>>(), vec![]);
    assert_eq!(depayloader.frame(FrameIndex(1)), None);

    // assert 3 packet, receive
    depayloader
        .handle_packet(&payloader.poll_packet().unwrap())
        .unwrap();
    assert!(depayloader.is_frame_known(FrameIndex(1)));
    assert!(depayloader.is_frame_available(FrameIndex(1)));
    assert_eq!(
        depayloader.known_frames().collect::<Vec<_>>(),
        vec![FrameIndex(1)]
    );
    assert_eq!(
        depayloader.available_frames().collect::<Vec<_>>(),
        vec![FrameIndex(1)]
    );

    let Some(VideoFrame {
        metadata,
        parsed_frame_type,
        buffers,
    }) = depayloader.frame(FrameIndex(1))
    else {
        panic!("expected Frame");
    };

    assert_eq!(metadata.frame_index, FrameIndex(1));
    assert_eq!(metadata.frame_type, ServerFrameType::Idr);
    assert_eq!(parsed_frame_type, FrameType::Idr);
    assert_eq!(metadata.timestamp, Duration::from_millis(0));
    assert_eq!(
        metadata.host_processing_latency,
        Some(expected_host_processing_latency)
    );

    let mut buffer = Vec::new();
    for VideoFrameBuffer { buffer_type, data } in buffers {
        assert_eq!(buffer_type, BufferType::PicData);
        buffer.extend_from_slice(data);
    }
    assert_eq!(buffer.as_slice(), expected_frame.as_slice());

    // test discard
    depayloader.discard_frame(FrameIndex(1));

    assert!(!depayloader.is_frame_known(FrameIndex(1)));
    assert!(!depayloader.is_frame_available(FrameIndex(1)));
    assert_eq!(depayloader.known_frames().collect::<Vec<_>>(), vec![]);
    assert_eq!(depayloader.available_frames().collect::<Vec<_>>(), vec![]);
    assert_eq!(depayloader.frame(FrameIndex(1)), None);
}

#[test]
fn fec_noparse() {
    let encryption = SunshineEncryption {
        aes_key: AesKey([0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15]),
        aes_iv: AesIv(0),
    };
    let server_version = sunshine_gen_7_431();
    let payload_size = 10;
    let packet_size = payload_size + video_header_size(true);
    let expected_host_processing_latency = Duration::from_millis(10);

    let expected_frame = [
        0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 9, 8, 7, 6, 5, 4, 3, 2, 1, 0, 0, 1,
    ];
    assert_eq!(expected_frame.len(), 30 - VideoFrameHeader::SIZE);

    let mut payloader = VideoPayloader::new(
        VideoPayloaderConfig {
            server_version,
            packet_size,
            fec: Some(VideoPayloaderFecConfig {
                min_required_fec_packets: 1,
                fec_percentage: 0,
            }),
            max_data_shards_per_block: 100,
            encryption: Some((encryption.aes_key, encryption.aes_iv)),
        },
        Arc::new(RustCryptoBackend),
    );
    payloader
        .push_frame(
            0,
            Some(expected_host_processing_latency),
            ServerFrameType::Idr,
            &expected_frame,
        )
        .unwrap();

    let mut depayloader = VideoDepayloader::new(
        VideoDepayloaderConfig {
            packet_size,
            // av1 doesn't get parsed
            format: VideoFormat::Av1Main8,
            server_version: sunshine_gen_7_431(),
            encryption: Some(encryption),
        },
        Arc::new(RustCryptoBackend),
    );

    // assert empty
    assert!(!depayloader.is_frame_known(FrameIndex(1)));
    assert!(!depayloader.is_frame_available(FrameIndex(1)));
    assert_eq!(depayloader.known_frames().collect::<Vec<_>>(), vec![]);
    assert_eq!(depayloader.available_frames().collect::<Vec<_>>(), vec![]);
    assert_eq!(depayloader.frame(FrameIndex(1)), None);

    // assert 1 packet
    depayloader
        .handle_packet(&payloader.poll_packet().unwrap())
        .unwrap();
    assert!(depayloader.is_frame_known(FrameIndex(1)));
    assert!(!depayloader.is_frame_available(FrameIndex(1)));
    assert_eq!(
        depayloader.known_frames().collect::<Vec<_>>(),
        vec![FrameIndex(1)]
    );
    assert_eq!(depayloader.available_frames().collect::<Vec<_>>(), vec![]);
    assert_eq!(depayloader.frame(FrameIndex(1)), None);

    // assert 2 packet
    depayloader
        .handle_packet(&payloader.poll_packet().unwrap())
        .unwrap();
    assert!(depayloader.is_frame_known(FrameIndex(1)));
    assert!(!depayloader.is_frame_available(FrameIndex(1)));
    assert_eq!(
        depayloader.known_frames().collect::<Vec<_>>(),
        vec![FrameIndex(1)]
    );
    assert_eq!(depayloader.available_frames().collect::<Vec<_>>(), vec![]);
    assert_eq!(depayloader.frame(FrameIndex(1)), None);

    // drop packet 3
    let _ = payloader.poll_packet().unwrap();

    // assert 4 packet, receive
    depayloader
        .handle_packet(&payloader.poll_packet().unwrap())
        .unwrap();

    assert!(depayloader.is_frame_known(FrameIndex(1)));
    assert!(depayloader.is_frame_available(FrameIndex(1)));
    assert_eq!(
        depayloader.known_frames().collect::<Vec<_>>(),
        vec![FrameIndex(1)]
    );
    assert_eq!(
        depayloader.available_frames().collect::<Vec<_>>(),
        vec![FrameIndex(1)]
    );
    let Some(VideoFrame {
        metadata,
        parsed_frame_type,
        buffers,
    }) = depayloader.frame(FrameIndex(1))
    else {
        panic!("expected Frame");
    };

    assert_eq!(metadata.frame_index, FrameIndex(1));
    assert_eq!(metadata.frame_type, ServerFrameType::Idr);
    assert_eq!(parsed_frame_type, FrameType::Idr);
    assert_eq!(metadata.timestamp, Duration::from_millis(0));
    assert_eq!(
        metadata.host_processing_latency,
        Some(expected_host_processing_latency)
    );

    let mut buffer = Vec::new();
    for VideoFrameBuffer { buffer_type, data } in buffers {
        assert_eq!(buffer_type, BufferType::PicData);
        buffer.extend_from_slice(data);
    }
    assert_eq!(buffer.as_slice(), expected_frame.as_slice());

    // test discard
    depayloader.discard_frame(FrameIndex(1));

    assert!(!depayloader.is_frame_known(FrameIndex(1)));
    assert!(!depayloader.is_frame_available(FrameIndex(1)));
    assert_eq!(depayloader.known_frames().collect::<Vec<_>>(), vec![]);
    assert_eq!(depayloader.available_frames().collect::<Vec<_>>(), vec![]);
    assert_eq!(depayloader.frame(FrameIndex(1)), None);
}

#[test]
fn udp_datagram() {
    const KEY: [u8; 16] = [
        0xb0, 0xb1, 0xb2, 0xb3, 0xb4, 0xb5, 0xb6, 0xb7, 0xb8, 0xb9, 0xba, 0xbb, 0xbc, 0xbd, 0xbe,
        0xbf,
    ];

    pub const PLAINTEXT: [u8; 80] = [
        0x90, 0x00, 0x00, 0x07, 0x00, 0x01, 0x5f, 0x90, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x07, 0x00, 0x00, 0x05, 0x00, 0x00, 0x00, 0x07, 0x00, 0x10, 0x00, 0x00, 0x00,
        0x40, 0x00, 0x01, 0x00, 0x00, 0x02, 0x30, 0x00, 0x00, 0x00, 0x08, 0x09, 0x0a, 0x0b, 0x0c,
        0x0d, 0x0e, 0x0f, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b,
        0x1c, 0x1d, 0x1e, 0x1f, 0x20, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27, 0x28, 0x29, 0x2a,
        0x2b, 0x2c, 0x2d, 0x2e, 0x2f,
    ];

    const DATAGRAM: [u8; 112] = [
        0x04, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x56, 0x05, 0x00, 0x00,
        0x00, 0xe8, 0x69, 0x72, 0xb0, 0x47, 0x36, 0x36, 0x5f, 0xee, 0x6c, 0x72, 0x9f, 0x27, 0x83,
        0xda, 0x95, 0x52, 0x92, 0xe2, 0x8b, 0xed, 0xc6, 0xbf, 0xcd, 0x85, 0xdd, 0x27, 0x60, 0xc6,
        0x9e, 0xfc, 0xe3, 0x04, 0x1a, 0x6e, 0x92, 0xca, 0x1e, 0xf8, 0xba, 0x39, 0xec, 0x7b, 0xcb,
        0x17, 0x6a, 0xc0, 0xc6, 0xf0, 0x4d, 0xbf, 0x8d, 0xed, 0x97, 0x58, 0x3f, 0x9a, 0x8c, 0x07,
        0x6d, 0x6e, 0xca, 0xd3, 0x40, 0x59, 0x10, 0xe0, 0xcb, 0xe1, 0xc0, 0xa2, 0x74, 0x16, 0x17,
        0x33, 0xac, 0xae, 0xe4, 0xe7, 0x1b, 0xed, 0x95, 0x71, 0x5c, 0x58, 0xdb, 0xa5, 0x0f, 0x57,
        0x5e, 0x79, 0xf3, 0x24, 0x76, 0xfc, 0x73,
    ];

    let expected_frame = &PLAINTEXT[(RtpVideoHeader::SIZE + VideoHeader::SIZE + 8)..];

    let mut depayloader = VideoDepayloader::new(
        VideoDepayloaderConfig {
            packet_size: DATAGRAM.len(),
            format: VideoFormat::Av1Main8,
            server_version: sunshine_gen_7_431(),
            encryption: Some(SunshineEncryption {
                aes_key: AesKey(KEY),
                aes_iv: AesIv(0),
            }),
        },
        Arc::new(RustCryptoBackend),
    );

    depayloader.handle_packet(&DATAGRAM).unwrap();

    let frame = depayloader.frame(FrameIndex(5)).unwrap();
    assert_eq!(frame.metadata.frame_type, ServerFrameType::Idr);

    let mut buffer = Vec::new();
    for VideoFrameBuffer { buffer_type, data } in frame.buffers {
        assert_eq!(buffer_type, BufferType::PicData);
        buffer.extend_from_slice(data);
    }
    assert_eq!(buffer.as_slice(), expected_frame);
}
