use std::{sync::Arc, time::Duration};

use crate::{
    ServerVersion,
    crypto::rustcrypto::RustCryptoBackend,
    stream::{
        AesIv, AesKey, SunshineEncryption,
        proto::video::{
            depayloader::{VideoDepayloader, VideoDepayloaderConfig},
            frame::VideoFrame,
            packet::{VideoFrameHeader, video_header_size},
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
