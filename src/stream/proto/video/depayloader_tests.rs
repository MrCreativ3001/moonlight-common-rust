use std::{iter, time::Duration};
use tracing::info;

use crate::{
    ServerVersion,
    stream::{
        proto::video::{
            depayloader::{VideoDepayloader, VideoDepayloaderConfig},
            frame::{VideoFrame, VideoFrameMetadata},
            packet::{FrameType, RtpVideoHeader, VideoFrameHeader, VideoHeader},
            payloader::{VideoPayloader, VideoPayloaderConfig, VideoPayloaderFecConfig},
        },
        video::{
            self, BufferType, FrameIndex, VideoDecodeUnitBuffers, VideoFormat, VideoFrameBuffer,
        },
    },
};

fn sunshine_gen_7_431() -> ServerVersion {
    ServerVersion::new(7, 1, 431, -1)
}

#[test]
fn nofec_noparse() {
    let server_version = sunshine_gen_7_431();
    let payload_size = 10;
    let expected_host_processing_latency = Duration::from_millis(10);

    let expected_frame = [
        0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 9, 8, 7, 6, 5, 4, 3, 2, 1, 0, 0, 1,
    ];
    assert_eq!(expected_frame.len(), 30 - VideoFrameHeader::SIZE);

    let mut payloader = VideoPayloader::new(VideoPayloaderConfig {
        server_version,
        packet_size: payload_size + VideoHeader::SIZE,
        fec: None,
        max_data_shards_per_block: 100,
    });
    payloader
        .push_frame(
            0,
            Some(expected_host_processing_latency),
            FrameType::Idr,
            &expected_frame,
        )
        .unwrap();

    let mut depayloader = VideoDepayloader::new(VideoDepayloaderConfig {
        packet_size: payload_size + VideoHeader::SIZE,
        // av1 doesn't get parsed
        format: VideoFormat::Av1Main8,
        server_version: sunshine_gen_7_431(),
    });

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
    assert_eq!(metadata.frame_type, FrameType::Idr);
    assert_eq!(parsed_frame_type, video::FrameType::Idr);
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
fn nofec_h264() {
    let server_version = sunshine_gen_7_431();
    let payload_size = 10;

    let mut depayloader = VideoDepayloader::new(VideoDepayloaderConfig {
        packet_size: payload_size + VideoHeader::SIZE,
        format: VideoFormat::H264,
        server_version: sunshine_gen_7_431(),
    });

    let expected_buffers = VideoDecodeUnitBuffers::from(vec![
        VideoFrameBuffer::<&[u8]> {
            buffer_type: BufferType::Sps,
            data: &[0u8, 0, 1, 0x67, 3, 4],
        },
        VideoFrameBuffer {
            buffer_type: BufferType::Pps,
            data: &[0, 0, 1, 0x68, 9, 9, 8, 7, 6, 5],
        },
        VideoFrameBuffer {
            // idr
            buffer_type: BufferType::PicData,
            data: &[0, 0, 1, 0x65, 0, 1],
        },
    ]);
    let expected_frame = expected_buffers
        .iter()
        .flat_map(|buf| buf.data)
        .copied()
        .collect::<Vec<_>>();

    let mut payloader = VideoPayloader::new(VideoPayloaderConfig {
        server_version,
        packet_size: payload_size + VideoHeader::SIZE,
        fec: None,
        max_data_shards_per_block: 100,
    });
    payloader
        .push_frame(0, None, FrameType::Idr, &expected_frame)
        .unwrap();

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

    // assert 3 packet
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
    assert_eq!(
        depayloader.frame(FrameIndex(1)),
        Some(VideoFrame {
            metadata: VideoFrameMetadata {
                frame_type: FrameType::Idr,
                frame_index: FrameIndex(1),
                timestamp: Duration::from_millis(0),
                host_processing_latency: None,
            },
            parsed_frame_type: video::FrameType::Idr,
            buffers: expected_buffers,
        })
    );
}

#[test]
fn nofec_h265() {
    let server_version = sunshine_gen_7_431();
    let payload_size = 15;

    let mut depayloader = VideoDepayloader::new(VideoDepayloaderConfig {
        packet_size: payload_size + VideoHeader::SIZE,
        format: VideoFormat::H265,
        server_version: sunshine_gen_7_431(),
    });

    let expected_buffers = VideoDecodeUnitBuffers::from(vec![
        VideoFrameBuffer::<&[u8]> {
            buffer_type: BufferType::Vps,
            data: &[0, 0, 1, 0x40, 1, 4, 1, 2, 3, 58, 67],
        },
        VideoFrameBuffer {
            buffer_type: BufferType::Sps,
            data: &[0, 0, 1, 0x42, 1, 4, 5, 56],
        },
        VideoFrameBuffer {
            buffer_type: BufferType::Pps,
            data: &[0, 0, 1, 0x44, 1, 6, 7, 33],
        },
        VideoFrameBuffer {
            // idr
            buffer_type: BufferType::PicData,
            data: &[0, 0, 1, 0x28, 1, 1, 8, 5, 38, 120],
        },
    ]);
    let expected_frame = expected_buffers
        .iter()
        .flat_map(|buf| buf.data)
        .copied()
        .collect::<Vec<_>>();

    assert_eq!(expected_frame.len(), 45 - VideoFrameHeader::SIZE);

    let mut payloader = VideoPayloader::new(VideoPayloaderConfig {
        server_version,
        packet_size: payload_size + VideoHeader::SIZE,
        fec: None,
        max_data_shards_per_block: 100,
    });
    payloader
        .push_frame(0, None, FrameType::Idr, &expected_frame)
        .unwrap();

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

    // assert 3 packet
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
    assert_eq!(
        depayloader.frame(FrameIndex(1)),
        Some(VideoFrame {
            metadata: VideoFrameMetadata {
                frame_type: FrameType::Idr,
                frame_index: FrameIndex(1),
                timestamp: Duration::from_millis(0),
                host_processing_latency: None,
            },
            parsed_frame_type: video::FrameType::Idr,
            buffers: expected_buffers,
        })
    );
}

#[test]
fn fec_noparse() {
    let server_version = sunshine_gen_7_431();
    let payload_size = 10;
    let expected_host_processing_latency = Duration::from_millis(10);

    let expected_frame = [
        0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 9, 8, 7, 6, 5, 4, 3, 2, 1, 0, 0, 1,
    ];
    assert_eq!(expected_frame.len(), 30 - VideoFrameHeader::SIZE);

    let mut payloader = VideoPayloader::new(VideoPayloaderConfig {
        server_version,
        packet_size: payload_size + VideoHeader::SIZE,
        fec: Some(VideoPayloaderFecConfig {
            min_required_fec_packets: 1,
            fec_percentage: 0,
        }),
        max_data_shards_per_block: 100,
    });
    payloader
        .push_frame(
            0,
            Some(expected_host_processing_latency),
            FrameType::Idr,
            &expected_frame,
        )
        .unwrap();

    let mut depayloader = VideoDepayloader::new(VideoDepayloaderConfig {
        packet_size: payload_size + VideoHeader::SIZE,
        // av1 doesn't get parsed
        format: VideoFormat::Av1Main8,
        server_version: sunshine_gen_7_431(),
    });

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
    assert_eq!(metadata.frame_type, FrameType::Idr);
    assert_eq!(parsed_frame_type, video::FrameType::Idr);
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
fn nofec_multiple_blocks() {
    // Define variables
    let packet_size = 1024;
    let payload_size = packet_size - VideoHeader::SIZE - RtpVideoHeader::SIZE;
    let shards_per_block = 3;
    let blocks = 3;

    // Build frame
    let frame_type = FrameType::Idr;
    let data: Vec<u8> = (0..(payload_size * (shards_per_block * blocks) - VideoFrameHeader::SIZE))
        .map(|x| (x % 255) as u8)
        .collect();

    // Create Payloader
    let mut payloader = VideoPayloader::new(VideoPayloaderConfig {
        server_version: sunshine_gen_7_431(),
        packet_size,
        max_data_shards_per_block: shards_per_block,
        fec: None,
    });
    payloader.push_frame(0, None, frame_type, &data).unwrap();

    // Create and test Depayloader
    let mut depayloader = VideoDepayloader::new(VideoDepayloaderConfig {
        packet_size,
        format: VideoFormat::Av1Main8,
        server_version: sunshine_gen_7_431(),
    });

    let mut packets = 0;
    let mut iter = iter::from_fn(|| payloader.poll_packet()).peekable();
    while let Some(packet) = iter.next() {
        packets += 1;

        depayloader.handle_packet(&packet).unwrap();

        if iter.peek().is_some() {
            assert!(!depayloader.is_frame_available(FrameIndex(1)));
        }
        assert!(depayloader.is_frame_known(FrameIndex(1)));
    }
    assert_eq!(packets, 9);

    assert!(depayloader.is_frame_known(FrameIndex(1)));
    assert!(depayloader.is_frame_available(FrameIndex(1)));

    let Some(frame) = depayloader.frame(FrameIndex(1)) else {
        panic!("expected Frame");
    };

    assert_eq!(frame.metadata.frame_index, FrameIndex(1));
    assert_eq!(frame.metadata.frame_type, frame_type);
    assert_eq!(frame.metadata.host_processing_latency, None);
    assert_eq!(frame.metadata.timestamp, Duration::ZERO);

    assert_eq!(frame.buffers.len(), 1);
    assert_eq!(frame.buffers[0].data, &data);
}

#[test]
fn fec_multiple_blocks() {
    // Define variables
    let packet_size = 1024;
    let payload_size = packet_size - VideoHeader::SIZE - RtpVideoHeader::SIZE;
    let shards_per_block = 3;
    let blocks = 3;

    // Build frame
    let frame_type = FrameType::Idr;
    let data: Vec<u8> = (0..(payload_size * (shards_per_block * blocks) - VideoFrameHeader::SIZE))
        .map(|x| (x % 255) as u8)
        .collect();

    // Create Payloader
    let mut payloader = VideoPayloader::new(VideoPayloaderConfig {
        server_version: sunshine_gen_7_431(),
        packet_size,
        max_data_shards_per_block: shards_per_block,
        fec: Some(VideoPayloaderFecConfig {
            min_required_fec_packets: 1,
            fec_percentage: 0,
        }),
    });
    payloader.push_frame(0, None, frame_type, &data).unwrap();

    // Create and test Depayloader
    let mut depayloader = VideoDepayloader::new(VideoDepayloaderConfig {
        packet_size,
        format: VideoFormat::Av1Main8,
        server_version: sunshine_gen_7_431(),
    });

    let mut packets = 0;
    let mut iter = iter::from_fn(|| payloader.poll_packet()).peekable();
    while let Some(packet) = iter.next() {
        packets += 1;
        if packets == 1 || packets == 5 || packets == 9 {
            // drop this packet
            // it should be a data packet
            continue;
        }
        info!(packet = %packets, "packet");

        depayloader.handle_packet(&packet).unwrap();

        if iter.peek().is_some() {
            assert!(!depayloader.is_frame_available(FrameIndex(1)));
        }
        assert!(depayloader.is_frame_known(FrameIndex(1)));
    }
    assert_eq!(packets, 12);

    assert!(depayloader.is_frame_known(FrameIndex(1)));
    assert!(depayloader.is_frame_available(FrameIndex(1)));

    let Some(frame) = depayloader.frame(FrameIndex(1)) else {
        panic!("expected Frame");
    };

    assert_eq!(frame.metadata.frame_index, FrameIndex(1));
    assert_eq!(frame.metadata.frame_type, frame_type);
    assert_eq!(frame.metadata.host_processing_latency, None);
    assert_eq!(frame.metadata.timestamp, Duration::ZERO);

    assert_eq!(frame.buffers.len(), 1);
    assert_eq!(frame.buffers[0].data, &data);
}
