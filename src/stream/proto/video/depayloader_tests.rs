use std::time::Duration;

use crate::{
    ServerVersion,
    stream::{
        proto::video::{
            depayloader::{VideoDepayloader, VideoDepayloaderConfig},
            frame::{VideoFrame, VideoFrameMetadata},
            packet::{
                FrameType, RtpVideoHeader, VIDEO_FLAG_EXTENSION, VideoFecInfo, VideoFrameHeader,
                VideoHeader, VideoHeaderExtraFlags, VideoHeaderFlags, VideoMultiFecBlocks,
            },
            payloader::{VideoPayloader, VideoPayloaderConfig, VideoPayloaderFecConfig},
            test::construct_packet,
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
fn fec_multiple_blocks() {
    let payload_size = 16;
    let data_shards_total = 3u32;
    let last_block_index = 2u8;
    let blocks = (last_block_index + 1) as usize;
    let shard_count = blocks * data_shards_total as usize;

    // Build a frame payload that spans exactly 9 full shards (3 blocks x 3 shards).
    let mut data =
        vec![0; VideoFrameHeader::SIZE + payload_size * shard_count - VideoFrameHeader::SIZE];
    let frame_header = VideoFrameHeader {
        header_type: 0x01,
        frame_type: FrameType::PFrame,
        host_processing_latency: 0,
        last_payload_len: payload_size as u16,
        reserved: [0; _],
    };
    frame_header.serialize(data[0..VideoFrameHeader::SIZE].as_mut_array().unwrap());
    for (i, byte) in data[VideoFrameHeader::SIZE..].iter_mut().enumerate() {
        *byte = (i % u8::MAX as usize) as u8;
    }
    let shards = data.chunks(payload_size).collect::<Vec<_>>();
    assert_eq!(shards.len(), shard_count);

    let mut depayloader = VideoDepayloader::new(VideoDepayloaderConfig {
        packet_size: payload_size + VideoHeader::SIZE,
        format: VideoFormat::Av1Main8,
        server_version: sunshine_gen_7_431(),
    });

    let mut sequence_number = 0;
    for block in 0..=last_block_index {
        for shard_index in 0..data_shards_total {
            let is_first = block == 0 && shard_index == 0;
            let is_last = block == last_block_index && shard_index == data_shards_total - 1;

            let flags = if is_first {
                VideoHeaderFlags::CONTAINS_VIDEO_DATA | VideoHeaderFlags::START_OF_FILE
            } else if is_last {
                VideoHeaderFlags::CONTAINS_VIDEO_DATA | VideoHeaderFlags::END_OF_FILE
            } else {
                VideoHeaderFlags::CONTAINS_VIDEO_DATA
            };

            let packet = construct_packet(
                RtpVideoHeader {
                    header: 0x80 | VIDEO_FLAG_EXTENSION,
                    packet_type: 0,
                    sequence_number,
                    timestamp: 0,
                    ssrc: 0,
                    reserved: [0; _],
                },
                VideoHeader {
                    stream_packet_index: sequence_number as u32,
                    frame_index: 1,
                    flags,
                    extra_flags: VideoHeaderExtraFlags::empty(),
                    multi_fec_flags: 0x10,
                    multi_fec_blocks: VideoMultiFecBlocks {
                        last_block_index,
                        current_block: block,
                        unused: 0,
                    },
                    fec_info: VideoFecInfo {
                        data_shards_total,
                        shard_index,
                        fec_percentage: 0,
                        unused: 0,
                    },
                },
                shards[block as usize * data_shards_total as usize + shard_index as usize],
            );

            depayloader.handle_packet(&packet).unwrap();
            sequence_number += 1;

            if !is_last {
                assert!(!depayloader.is_frame_available(FrameIndex(1)));
            }
            assert!(depayloader.is_frame_known(FrameIndex(1)));
        }
    }

    assert!(depayloader.is_frame_known(FrameIndex(1)));
    assert!(depayloader.is_frame_available(FrameIndex(1)));

    let Some(VideoFrame { metadata, .. }) = depayloader.frame(FrameIndex(1)) else {
        panic!("expected Frame");
    };
    assert_eq!(metadata.frame_index, FrameIndex(1));
}
