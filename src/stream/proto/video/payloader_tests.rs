use std::array;

use fec_rs::ReedSolomon;
use tracing::info;

use crate::{
    ServerVersion,
    stream::proto::video::{
        depayloader::create_video_reed_solomon,
        packet::{
            FrameType, RtpVideoHeader, VIDEO_FLAG_EXTENSION, VideoFecInfo, VideoFrameHeader,
            VideoHeader, VideoHeaderExtraFlags, VideoHeaderFlags, VideoMultiFecBlocks,
            fec_percentage_from,
        },
        payloader::{VideoPayloader, VideoPayloaderConfig, VideoPayloaderFecConfig},
        test::construct_packet,
    },
};

fn sunshine_gen_7_431() -> ServerVersion {
    ServerVersion::new(7, 1, 431, -1)
}

#[test]
fn nofec_empty() {
    // make sure this doesn't crash
    let mut payloader = VideoPayloader::new(VideoPayloaderConfig {
        server_version: sunshine_gen_7_431(),
        packet_size: 1024,
        fec: None,
        max_data_shards_per_block: 100,
    });

    payloader
        .push_frame(0, None, FrameType::PFrame, &[])
        .unwrap();

    while payloader.poll_packet().is_some() {}
}

#[test]
fn nofec_frame_length_1() {
    // make sure this doesn't crash
    let mut payloader = VideoPayloader::new(VideoPayloaderConfig {
        server_version: sunshine_gen_7_431(),
        packet_size: 1024,
        fec: None,
        max_data_shards_per_block: 100,
    });

    payloader
        .push_frame(0, None, FrameType::PFrame, &[0])
        .unwrap();

    while payloader.poll_packet().is_some() {}
}

#[test]
fn nofec() {
    let mut data: [u8; 128 + 512] = array::from_fn(|i| (i % u8::MAX as usize) as u8);
    // Copy the frame header
    let frame_header = VideoFrameHeader {
        header_type: 0x01,
        frame_type: FrameType::PFrame,
        host_processing_latency: 0,
        // The last payload should have 8 bytes (VideoFrameHeader::SIZE) because it's always appended at the beginning
        last_payload_len: VideoFrameHeader::SIZE as u16,
        reserved: [0; _],
    };
    frame_header.serialize(data[0..VideoFrameHeader::SIZE].as_mut_array().unwrap());
    // Add zero padding
    data[(VideoFrameHeader::SIZE + 512)..].fill(0);

    let data_shards_total = 5;

    let mut payloader = VideoPayloader::new(VideoPayloaderConfig {
        server_version: sunshine_gen_7_431(),
        packet_size: 128 + VideoHeader::SIZE,
        fec: None,
        max_data_shards_per_block: 100,
    });

    payloader
        .push_frame(
            0,
            None,
            FrameType::PFrame,
            &data[VideoFrameHeader::SIZE..(VideoFrameHeader::SIZE + 512)],
        )
        .unwrap();

    assert_eq!(
        payloader.poll_packet().as_deref(),
        Some(
            construct_packet(
                RtpVideoHeader {
                    header: 0x80 | VIDEO_FLAG_EXTENSION,
                    packet_type: 0,
                    sequence_number: 0,
                    timestamp: 0,
                    ssrc: 0,
                    reserved: [0; _],
                },
                VideoHeader {
                    stream_packet_index: 0u32 << 8,
                    frame_index: 1,
                    flags: VideoHeaderFlags::CONTAINS_VIDEO_DATA | VideoHeaderFlags::START_OF_FILE,
                    extra_flags: VideoHeaderExtraFlags::empty(),
                    multi_fec_flags: 0x10,
                    multi_fec_blocks: VideoMultiFecBlocks {
                        last_block_index: 0,
                        current_block: 0,
                        unused: 0
                    },
                    fec_info: VideoFecInfo {
                        data_shards_total,
                        shard_index: 0,
                        fec_percentage: 0,
                        unused: 0,
                    }
                },
                &data[0..128]
            )
            .as_slice()
        ),
    );
    assert_eq!(
        payloader.poll_packet().as_deref(),
        Some(
            construct_packet(
                RtpVideoHeader {
                    header: 0x80 | VIDEO_FLAG_EXTENSION,
                    packet_type: 0,
                    sequence_number: 1,
                    timestamp: 0,
                    ssrc: 0,
                    reserved: [0; _],
                },
                VideoHeader {
                    stream_packet_index: 1u32 << 8,
                    frame_index: 1,
                    flags: VideoHeaderFlags::CONTAINS_VIDEO_DATA,
                    extra_flags: VideoHeaderExtraFlags::empty(),
                    multi_fec_flags: 0x10,
                    multi_fec_blocks: VideoMultiFecBlocks {
                        last_block_index: 0,
                        current_block: 0,
                        unused: 0
                    },
                    fec_info: VideoFecInfo {
                        data_shards_total,
                        shard_index: 1,
                        fec_percentage: 0,
                        unused: 0,
                    }
                },
                &data[128..256]
            )
            .as_slice()
        ),
    );
    assert_eq!(
        payloader.poll_packet().as_deref(),
        Some(
            construct_packet(
                RtpVideoHeader {
                    header: 0x80 | VIDEO_FLAG_EXTENSION,
                    packet_type: 0,
                    sequence_number: 2,
                    timestamp: 0,
                    ssrc: 0,
                    reserved: [0; _],
                },
                VideoHeader {
                    stream_packet_index: 2u32 << 8,
                    frame_index: 1,
                    flags: VideoHeaderFlags::CONTAINS_VIDEO_DATA,
                    extra_flags: VideoHeaderExtraFlags::empty(),
                    multi_fec_flags: 0x10,
                    multi_fec_blocks: VideoMultiFecBlocks {
                        last_block_index: 0,
                        current_block: 0,
                        unused: 0
                    },
                    fec_info: VideoFecInfo {
                        data_shards_total,
                        shard_index: 2,
                        fec_percentage: 0,
                        unused: 0,
                    }
                },
                &data[256..384]
            )
            .as_slice()
        ),
    );
    assert_eq!(
        payloader.poll_packet().as_deref(),
        Some(
            construct_packet(
                RtpVideoHeader {
                    header: 0x80 | VIDEO_FLAG_EXTENSION,
                    packet_type: 0,
                    sequence_number: 3,
                    timestamp: 0,
                    ssrc: 0,
                    reserved: [0; _],
                },
                VideoHeader {
                    stream_packet_index: 3u32 << 8,
                    frame_index: 1,
                    flags: VideoHeaderFlags::CONTAINS_VIDEO_DATA,
                    extra_flags: VideoHeaderExtraFlags::empty(),
                    multi_fec_flags: 0x10,
                    multi_fec_blocks: VideoMultiFecBlocks {
                        last_block_index: 0,
                        current_block: 0,
                        unused: 0
                    },
                    fec_info: VideoFecInfo {
                        data_shards_total,
                        shard_index: 3,
                        fec_percentage: 0,
                        unused: 0,
                    }
                },
                &data[384..512]
            )
            .as_slice()
        ),
    );
    assert_eq!(
        payloader.poll_packet().as_deref(),
        Some(
            construct_packet(
                RtpVideoHeader {
                    header: 0x80 | VIDEO_FLAG_EXTENSION,
                    packet_type: 0,
                    sequence_number: 4,
                    timestamp: 0,
                    ssrc: 0,
                    reserved: [0; _],
                },
                VideoHeader {
                    stream_packet_index: 4u32 << 8,
                    frame_index: 1,
                    flags: VideoHeaderFlags::CONTAINS_VIDEO_DATA | VideoHeaderFlags::END_OF_FILE,
                    extra_flags: VideoHeaderExtraFlags::empty(),
                    multi_fec_flags: 0x10,
                    multi_fec_blocks: VideoMultiFecBlocks {
                        last_block_index: 0,
                        current_block: 0,
                        unused: 0
                    },
                    fec_info: VideoFecInfo {
                        data_shards_total,
                        shard_index: 4,
                        fec_percentage: 0,
                        unused: 0,
                    }
                },
                &data[512..640]
            )
            .as_slice()
        ),
    );
    assert_eq!(payloader.poll_packet(), None);
}

fn generate_frame_payload(
    frame: &[u8],
    host_processing_latency: u16,
    payload_size: usize,
) -> Vec<u8> {
    let full_payload_len = VideoFrameHeader::SIZE + frame.len();
    let padded_len = full_payload_len.div_ceil(payload_size) * payload_size;
    let mut data = vec![0; padded_len];

    let last_payload_len = if full_payload_len.is_multiple_of(frame.len()) {
        payload_size
    } else {
        full_payload_len % frame.len()
    };

    // Copy the frame header
    let frame_header = VideoFrameHeader {
        header_type: 0x01,
        frame_type: FrameType::PFrame,
        host_processing_latency,
        last_payload_len: last_payload_len as u16,
        reserved: [0; _],
    };
    frame_header.serialize(data[0..VideoFrameHeader::SIZE].as_mut_array().unwrap());

    // Copy Frame
    data[VideoFrameHeader::SIZE..full_payload_len].copy_from_slice(frame);

    // Add zero padding
    data[full_payload_len..].fill(0);

    data
}

#[test]
fn fec() {
    let payload_size = 128;

    let frame: [u8; 512] = array::from_fn(|i| (i % u8::MAX as usize) as u8);
    let full_payload = generate_frame_payload(&frame, 0, payload_size);

    let data_shards_total = 5u32;
    let parity_shard_count = 2;
    let fec_percentage = fec_percentage_from(data_shards_total as usize, parity_shard_count) as u32;

    let data_shards = full_payload.chunks(payload_size).collect::<Vec<_>>();
    let mut fec_data = vec![vec![0; payload_size]; 2];

    let reed_solomon = create_video_reed_solomon(data_shards_total as usize, parity_shard_count);
    reed_solomon
        .encode_sep(&data_shards, &mut fec_data)
        .unwrap();

    let mut payloader = VideoPayloader::new(VideoPayloaderConfig {
        server_version: sunshine_gen_7_431(),
        packet_size: payload_size + VideoHeader::SIZE,
        fec: Some(VideoPayloaderFecConfig {
            fec_percentage: 0,
            min_required_fec_packets: parity_shard_count,
        }),
        max_data_shards_per_block: 100,
    });

    payloader
        .push_frame(0, None, FrameType::PFrame, &frame)
        .unwrap();

    assert_eq!(
        payloader.poll_packet().as_deref(),
        Some(
            construct_packet(
                RtpVideoHeader {
                    header: 0x80 | VIDEO_FLAG_EXTENSION,
                    packet_type: 0,
                    sequence_number: 0,
                    timestamp: 0,
                    ssrc: 0,
                    reserved: [0; _],
                },
                VideoHeader {
                    stream_packet_index: 0u32 << 8,
                    frame_index: 1,
                    flags: VideoHeaderFlags::CONTAINS_VIDEO_DATA | VideoHeaderFlags::START_OF_FILE,
                    extra_flags: VideoHeaderExtraFlags::empty(),
                    multi_fec_flags: 0x10,
                    multi_fec_blocks: VideoMultiFecBlocks {
                        last_block_index: 0,
                        current_block: 0,
                        unused: 0
                    },
                    fec_info: VideoFecInfo {
                        data_shards_total,
                        shard_index: 0,
                        fec_percentage,
                        unused: 0,
                    }
                },
                &full_payload[0..128]
            )
            .as_slice()
        ),
    );
    assert_eq!(
        payloader.poll_packet().as_deref(),
        Some(
            construct_packet(
                RtpVideoHeader {
                    header: 0x80 | VIDEO_FLAG_EXTENSION,
                    packet_type: 0,
                    sequence_number: 1,
                    timestamp: 0,
                    ssrc: 0,
                    reserved: [0; _],
                },
                VideoHeader {
                    stream_packet_index: 1u32 << 8,
                    frame_index: 1,
                    flags: VideoHeaderFlags::CONTAINS_VIDEO_DATA,
                    extra_flags: VideoHeaderExtraFlags::empty(),
                    multi_fec_flags: 0x10,
                    multi_fec_blocks: VideoMultiFecBlocks {
                        last_block_index: 0,
                        current_block: 0,
                        unused: 0
                    },
                    fec_info: VideoFecInfo {
                        data_shards_total,
                        shard_index: 1,
                        fec_percentage,
                        unused: 0,
                    }
                },
                &full_payload[128..256]
            )
            .as_slice()
        ),
    );
    assert_eq!(
        payloader.poll_packet().as_deref(),
        Some(
            construct_packet(
                RtpVideoHeader {
                    header: 0x80 | VIDEO_FLAG_EXTENSION,
                    packet_type: 0,
                    sequence_number: 2,
                    timestamp: 0,
                    ssrc: 0,
                    reserved: [0; _],
                },
                VideoHeader {
                    stream_packet_index: 2u32 << 8,
                    frame_index: 1,
                    flags: VideoHeaderFlags::CONTAINS_VIDEO_DATA,
                    extra_flags: VideoHeaderExtraFlags::empty(),
                    multi_fec_flags: 0x10,
                    multi_fec_blocks: VideoMultiFecBlocks {
                        last_block_index: 0,
                        current_block: 0,
                        unused: 0
                    },
                    fec_info: VideoFecInfo {
                        data_shards_total,
                        shard_index: 2,
                        fec_percentage,
                        unused: 0,
                    }
                },
                &full_payload[256..384]
            )
            .as_slice()
        ),
    );
    assert_eq!(
        payloader.poll_packet().as_deref(),
        Some(
            construct_packet(
                RtpVideoHeader {
                    header: 0x80 | VIDEO_FLAG_EXTENSION,
                    packet_type: 0,
                    sequence_number: 3,
                    timestamp: 0,
                    ssrc: 0,
                    reserved: [0; _],
                },
                VideoHeader {
                    stream_packet_index: 3u32 << 8,
                    frame_index: 1,
                    flags: VideoHeaderFlags::CONTAINS_VIDEO_DATA,
                    extra_flags: VideoHeaderExtraFlags::empty(),
                    multi_fec_flags: 0x10,
                    multi_fec_blocks: VideoMultiFecBlocks {
                        last_block_index: 0,
                        current_block: 0,
                        unused: 0
                    },
                    fec_info: VideoFecInfo {
                        data_shards_total,
                        shard_index: 3,
                        fec_percentage,
                        unused: 0,
                    }
                },
                &full_payload[384..512]
            )
            .as_slice()
        ),
    );
    assert_eq!(
        payloader.poll_packet().as_deref(),
        Some(
            construct_packet(
                RtpVideoHeader {
                    header: 0x80 | VIDEO_FLAG_EXTENSION,
                    packet_type: 0,
                    sequence_number: 4,
                    timestamp: 0,
                    ssrc: 0,
                    reserved: [0; _],
                },
                VideoHeader {
                    stream_packet_index: 4u32 << 8,
                    frame_index: 1,
                    flags: VideoHeaderFlags::CONTAINS_VIDEO_DATA | VideoHeaderFlags::END_OF_FILE,
                    extra_flags: VideoHeaderExtraFlags::empty(),
                    multi_fec_flags: 0x10,
                    multi_fec_blocks: VideoMultiFecBlocks {
                        last_block_index: 0,
                        current_block: 0,
                        unused: 0
                    },
                    fec_info: VideoFecInfo {
                        data_shards_total,
                        shard_index: 4,
                        fec_percentage,
                        unused: 0,
                    }
                },
                &full_payload[512..640]
            )
            .as_slice()
        ),
    );
    assert_eq!(
        payloader.poll_packet().as_deref(),
        Some(
            construct_packet(
                RtpVideoHeader {
                    header: 0x80 | VIDEO_FLAG_EXTENSION,
                    packet_type: 0,
                    sequence_number: 5,
                    timestamp: 0,
                    ssrc: 0,
                    reserved: [0; _],
                },
                VideoHeader {
                    stream_packet_index: 5u32 << 8,
                    frame_index: 1,
                    flags: VideoHeaderFlags::CONTAINS_VIDEO_DATA,
                    extra_flags: VideoHeaderExtraFlags::empty(),
                    multi_fec_flags: 0x10,
                    multi_fec_blocks: VideoMultiFecBlocks {
                        last_block_index: 0,
                        current_block: 0,
                        unused: 0
                    },
                    fec_info: VideoFecInfo {
                        data_shards_total,
                        shard_index: 5,
                        fec_percentage,
                        unused: 0,
                    }
                },
                &fec_data[0]
            )
            .as_slice()
        ),
    );
    assert_eq!(
        payloader.poll_packet().as_deref(),
        Some(
            construct_packet(
                RtpVideoHeader {
                    header: 0x80 | VIDEO_FLAG_EXTENSION,
                    packet_type: 0,
                    sequence_number: 6,
                    timestamp: 0,
                    ssrc: 0,
                    reserved: [0; _],
                },
                VideoHeader {
                    stream_packet_index: 6u32 << 8,
                    frame_index: 1,
                    flags: VideoHeaderFlags::CONTAINS_VIDEO_DATA,
                    extra_flags: VideoHeaderExtraFlags::empty(),
                    multi_fec_flags: 0x10,
                    multi_fec_blocks: VideoMultiFecBlocks {
                        last_block_index: 0,
                        current_block: 0,
                        unused: 0
                    },
                    fec_info: VideoFecInfo {
                        data_shards_total,
                        shard_index: 6,
                        fec_percentage,
                        unused: 0,
                    }
                },
                &fec_data[1]
            )
            .as_slice()
        ),
    );
    assert_eq!(payloader.poll_packet().as_deref(), None);
}

#[test]
fn nofec_packet_size_8() {
    let payload_size = 8;
    let data_shards_total = 2;
    let fec_percentage = 0;

    let mut expected_header_bytes = [0; 8];
    let header = VideoFrameHeader {
        header_type: 0x01,
        frame_type: FrameType::PFrame,
        host_processing_latency: 0,
        last_payload_len: payload_size as u16,
        reserved: [0; _],
    };
    header.serialize(&mut expected_header_bytes);

    let expected_frame = [0, 1, 2, 3, 4, 5, 6, 7];

    let expected_packet1 = construct_packet(
        RtpVideoHeader {
            header: 0x80 | VIDEO_FLAG_EXTENSION,
            packet_type: 0,
            sequence_number: 0,
            timestamp: 0,
            ssrc: 0,
            reserved: [0; _],
        },
        VideoHeader {
            stream_packet_index: 0u32 << 8,
            frame_index: 1,
            flags: VideoHeaderFlags::CONTAINS_VIDEO_DATA | VideoHeaderFlags::START_OF_FILE,
            extra_flags: VideoHeaderExtraFlags::empty(),
            multi_fec_flags: 0x10,
            multi_fec_blocks: VideoMultiFecBlocks {
                last_block_index: 0,
                current_block: 0,
                unused: 0,
            },
            fec_info: VideoFecInfo {
                data_shards_total,
                shard_index: 0,
                fec_percentage,
                unused: 0,
            },
        },
        &expected_header_bytes,
    );
    let expected_packet2 = construct_packet(
        RtpVideoHeader {
            header: 0x80 | VIDEO_FLAG_EXTENSION,
            packet_type: 0,
            sequence_number: 1,
            timestamp: 0,
            ssrc: 0,
            reserved: [0; _],
        },
        VideoHeader {
            stream_packet_index: 1u32 << 8,
            frame_index: 1,
            flags: VideoHeaderFlags::CONTAINS_VIDEO_DATA | VideoHeaderFlags::END_OF_FILE,
            extra_flags: VideoHeaderExtraFlags::empty(),
            multi_fec_flags: 0x10,
            multi_fec_blocks: VideoMultiFecBlocks {
                last_block_index: 0,
                current_block: 0,
                unused: 0,
            },
            fec_info: VideoFecInfo {
                data_shards_total,
                shard_index: 1,
                fec_percentage,
                unused: 0,
            },
        },
        &expected_frame[0..payload_size],
    );
    assert_eq!(
        expected_packet1.len(),
        RtpVideoHeader::SIZE + VideoHeader::SIZE + payload_size
    );
    assert_eq!(expected_packet1.len(), expected_packet2.len());

    let mut payloader = VideoPayloader::new(VideoPayloaderConfig {
        server_version: sunshine_gen_7_431(),
        packet_size: VideoHeader::SIZE + payload_size,
        fec: None,
        max_data_shards_per_block: 100,
    });

    payloader
        .push_frame(0, None, FrameType::PFrame, &expected_frame)
        .unwrap();

    assert_eq!(
        payloader.poll_packet().as_deref(),
        Some(expected_packet1.as_slice())
    );
    assert_eq!(
        payloader.poll_packet().as_deref(),
        Some(expected_packet2.as_slice())
    );
    assert_eq!(payloader.poll_packet().as_deref(), None);
}

#[test]
fn nofec_packet_size_9() {
    let payload_size = 9;
    let data_shards_total = 2;
    let fec_percentage = 0;

    let mut expected_header_bytes = [0; 9];
    let header = VideoFrameHeader {
        header_type: 0x01,
        frame_type: FrameType::PFrame,
        host_processing_latency: 0,
        last_payload_len: payload_size as u16,
        reserved: [0; _],
    };
    header.serialize(expected_header_bytes[0..8].as_mut_array().unwrap());
    expected_header_bytes[8] = 1;

    let expected_frame = [1, 0, 1, 2, 3, 4, 5, 6, 7, 8];

    let expected_packet1 = construct_packet(
        RtpVideoHeader {
            header: 0x80 | VIDEO_FLAG_EXTENSION,
            packet_type: 0,
            sequence_number: 0,
            timestamp: 0,
            ssrc: 0,
            reserved: [0; _],
        },
        VideoHeader {
            stream_packet_index: 0u32 << 8,
            frame_index: 1,
            flags: VideoHeaderFlags::CONTAINS_VIDEO_DATA | VideoHeaderFlags::START_OF_FILE,
            extra_flags: VideoHeaderExtraFlags::empty(),
            multi_fec_flags: 0x10,
            multi_fec_blocks: VideoMultiFecBlocks {
                last_block_index: 0,
                current_block: 0,
                unused: 0,
            },
            fec_info: VideoFecInfo {
                data_shards_total,
                shard_index: 0,
                fec_percentage,
                unused: 0,
            },
        },
        &expected_header_bytes,
    );
    let expected_packet2 = construct_packet(
        RtpVideoHeader {
            header: 0x80 | VIDEO_FLAG_EXTENSION,
            packet_type: 0,
            sequence_number: 1,
            timestamp: 0,
            ssrc: 0,
            reserved: [0; _],
        },
        VideoHeader {
            stream_packet_index: 1u32 << 8,
            frame_index: 1,
            flags: VideoHeaderFlags::CONTAINS_VIDEO_DATA | VideoHeaderFlags::END_OF_FILE,
            extra_flags: VideoHeaderExtraFlags::empty(),
            multi_fec_flags: 0x10,
            multi_fec_blocks: VideoMultiFecBlocks {
                last_block_index: 0,
                current_block: 0,
                unused: 0,
            },
            fec_info: VideoFecInfo {
                data_shards_total,
                shard_index: 1,
                fec_percentage,
                unused: 0,
            },
        },
        &expected_frame[1..(1 + payload_size)],
    );
    assert_eq!(
        expected_packet1.len(),
        RtpVideoHeader::SIZE + VideoHeader::SIZE + payload_size
    );
    assert_eq!(expected_packet1.len(), expected_packet2.len());

    let mut payloader = VideoPayloader::new(VideoPayloaderConfig {
        server_version: sunshine_gen_7_431(),
        packet_size: VideoHeader::SIZE + payload_size,
        fec: None,
        max_data_shards_per_block: 100,
    });

    payloader
        .push_frame(0, None, FrameType::PFrame, &expected_frame)
        .unwrap();

    assert_eq!(
        payloader.poll_packet().as_deref(),
        Some(expected_packet1.as_slice())
    );
    assert_eq!(
        payloader.poll_packet().as_deref(),
        Some(expected_packet2.as_slice())
    );
    assert_eq!(payloader.poll_packet(), None);
}

#[test]
fn nofec_packet_size_10() {
    let payload_size = 10;
    let data_shards_total = 2;
    let fec_percentage = 0;

    let mut expected_header_bytes = [0; 10];
    let header = VideoFrameHeader {
        header_type: 0x01,
        frame_type: FrameType::PFrame,
        host_processing_latency: 0,
        last_payload_len: payload_size as u16,
        reserved: [0; _],
    };
    header.serialize(expected_header_bytes[0..8].as_mut_array().unwrap());
    expected_header_bytes[8] = 2;
    expected_header_bytes[9] = 1;

    let expected_frame = [2, 1, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9];

    let expected_packet1 = construct_packet(
        RtpVideoHeader {
            header: 0x80 | VIDEO_FLAG_EXTENSION,
            packet_type: 0,
            sequence_number: 0,
            timestamp: 0,
            ssrc: 0,
            reserved: [0; _],
        },
        VideoHeader {
            stream_packet_index: 0u32 << 8,
            frame_index: 1,
            flags: VideoHeaderFlags::CONTAINS_VIDEO_DATA | VideoHeaderFlags::START_OF_FILE,
            extra_flags: VideoHeaderExtraFlags::empty(),
            multi_fec_flags: 0x10,
            multi_fec_blocks: VideoMultiFecBlocks {
                last_block_index: 0,
                current_block: 0,
                unused: 0,
            },
            fec_info: VideoFecInfo {
                data_shards_total,
                shard_index: 0,
                fec_percentage,
                unused: 0,
            },
        },
        &expected_header_bytes,
    );
    let expected_packet2 = construct_packet(
        RtpVideoHeader {
            header: 0x80 | VIDEO_FLAG_EXTENSION,
            packet_type: 0,
            sequence_number: 1,
            timestamp: 0,
            ssrc: 0,
            reserved: [0; _],
        },
        VideoHeader {
            stream_packet_index: 1u32 << 8,
            frame_index: 1,
            flags: VideoHeaderFlags::CONTAINS_VIDEO_DATA | VideoHeaderFlags::END_OF_FILE,
            extra_flags: VideoHeaderExtraFlags::empty(),
            multi_fec_flags: 0x10,
            multi_fec_blocks: VideoMultiFecBlocks {
                last_block_index: 0,
                current_block: 0,
                unused: 0,
            },
            fec_info: VideoFecInfo {
                data_shards_total,
                shard_index: 1,
                fec_percentage,
                unused: 0,
            },
        },
        &expected_frame[2..(2 + payload_size)],
    );
    assert_eq!(
        expected_packet1.len(),
        RtpVideoHeader::SIZE + VideoHeader::SIZE + payload_size
    );
    assert_eq!(expected_packet1.len(), expected_packet2.len());

    let mut payloader = VideoPayloader::new(VideoPayloaderConfig {
        server_version: sunshine_gen_7_431(),
        packet_size: VideoHeader::SIZE + payload_size,
        fec: None,
        max_data_shards_per_block: 100,
    });

    payloader
        .push_frame(0, None, FrameType::PFrame, &expected_frame)
        .unwrap();

    assert_eq!(
        payloader.poll_packet().as_deref(),
        Some(expected_packet1.as_slice())
    );
    assert_eq!(
        payloader.poll_packet().as_deref(),
        Some(expected_packet2.as_slice())
    );
    assert_eq!(payloader.poll_packet(), None);
}

#[test]
fn fec_packet_size_10() {
    let payload_size = 10;
    let data_shards_total = 2u32;
    let fec_percentage = fec_percentage_from(data_shards_total as usize, 1) as u32;

    let mut expected_header_bytes = [0; 10];
    let header = VideoFrameHeader {
        header_type: 0x01,
        frame_type: FrameType::PFrame,
        host_processing_latency: 0,
        last_payload_len: payload_size as u16,
        reserved: [0; _],
    };
    header.serialize(expected_header_bytes[0..8].as_mut_array().unwrap());
    expected_header_bytes[8] = 2;
    expected_header_bytes[9] = 1;

    let expected_frame = [2, 1, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9];

    let expected_packet1 = construct_packet(
        RtpVideoHeader {
            header: 0x80 | VIDEO_FLAG_EXTENSION,
            packet_type: 0,
            sequence_number: 0,
            timestamp: 0,
            ssrc: 0,
            reserved: [0; _],
        },
        VideoHeader {
            stream_packet_index: 0u32 << 8,
            frame_index: 1,
            flags: VideoHeaderFlags::CONTAINS_VIDEO_DATA | VideoHeaderFlags::START_OF_FILE,
            extra_flags: VideoHeaderExtraFlags::empty(),
            multi_fec_flags: 0x10,
            multi_fec_blocks: VideoMultiFecBlocks {
                last_block_index: 0,
                current_block: 0,
                unused: 0,
            },
            fec_info: VideoFecInfo {
                data_shards_total,
                shard_index: 0,
                fec_percentage,
                unused: 0,
            },
        },
        &expected_header_bytes,
    );
    let expected_packet2 = construct_packet(
        RtpVideoHeader {
            header: 0x80 | VIDEO_FLAG_EXTENSION,
            packet_type: 0,
            sequence_number: 1,
            timestamp: 0,
            ssrc: 0,
            reserved: [0; _],
        },
        VideoHeader {
            stream_packet_index: 1u32 << 8,
            frame_index: 1,
            flags: VideoHeaderFlags::CONTAINS_VIDEO_DATA | VideoHeaderFlags::END_OF_FILE,
            extra_flags: VideoHeaderExtraFlags::empty(),
            multi_fec_flags: 0x10,
            multi_fec_blocks: VideoMultiFecBlocks {
                last_block_index: 0,
                current_block: 0,
                unused: 0,
            },
            fec_info: VideoFecInfo {
                data_shards_total,
                shard_index: 1,
                fec_percentage,
                unused: 0,
            },
        },
        &expected_frame[2..(2 + payload_size)],
    );

    // Generate fec packet
    let reed_solomon = ReedSolomon::new(2, 1).unwrap();
    let mut fec_data = vec![0; expected_packet1.len() - (RtpVideoHeader::SIZE + VideoHeader::SIZE)];
    reed_solomon
        .encode_sep(
            &[
                &expected_packet1[RtpVideoHeader::SIZE + VideoHeader::SIZE..],
                &expected_packet2[RtpVideoHeader::SIZE + VideoHeader::SIZE..],
            ],
            &mut [&mut fec_data],
        )
        .unwrap();

    let expected_packet3 = construct_packet(
        RtpVideoHeader {
            header: 0x80 | VIDEO_FLAG_EXTENSION,
            packet_type: 0,
            sequence_number: 2,
            timestamp: 0,
            ssrc: 0,
            reserved: [0; _],
        },
        VideoHeader {
            stream_packet_index: 2u32 << 8,
            frame_index: 1,
            flags: VideoHeaderFlags::CONTAINS_VIDEO_DATA,
            extra_flags: VideoHeaderExtraFlags::empty(),
            multi_fec_flags: 0x10,
            multi_fec_blocks: VideoMultiFecBlocks {
                last_block_index: 0,
                current_block: 0,
                unused: 0,
            },
            fec_info: VideoFecInfo {
                data_shards_total,
                shard_index: 2,
                fec_percentage,
                unused: 0,
            },
        },
        &fec_data,
    );

    assert_eq!(
        expected_packet1.len(),
        RtpVideoHeader::SIZE + VideoHeader::SIZE + payload_size
    );
    assert_eq!(expected_packet1.len(), expected_packet2.len());
    assert_eq!(expected_packet1.len(), expected_packet3.len());

    // Test payloader
    let mut payloader = VideoPayloader::new(VideoPayloaderConfig {
        server_version: sunshine_gen_7_431(),
        packet_size: VideoHeader::SIZE + payload_size,
        fec: None,
        max_data_shards_per_block: 100,
    });
    payloader.set_fec_config(Some(VideoPayloaderFecConfig {
        min_required_fec_packets: 1,
        fec_percentage: 0,
    }));

    payloader
        .push_frame(0, None, FrameType::PFrame, &expected_frame)
        .unwrap();

    assert_eq!(
        payloader.poll_packet().as_deref(),
        Some(expected_packet1.as_slice())
    );
    assert_eq!(
        payloader.poll_packet().as_deref(),
        Some(expected_packet2.as_slice())
    );
    assert_eq!(
        payloader.poll_packet().as_deref(),
        Some(expected_packet3.as_slice())
    );
    assert_eq!(payloader.poll_packet().as_deref(), None);
}

#[test]
fn nofec_multiblock() {
    let payload_size = 16;
    let data_shards_total = 3u32;
    let last_block_index = 2u8;
    let blocks = (last_block_index + 1) as usize;
    let shard_count = blocks * data_shards_total as usize;

    // -- Build a frame payload that spans exactly 9 full shards (3 blocks x 3 shards).
    let mut data = vec![0; payload_size * shard_count];
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

    // -- Create payloader
    let mut payloader = VideoPayloader::new(VideoPayloaderConfig {
        packet_size: payload_size + VideoHeader::SIZE,
        fec: None,
        max_data_shards_per_block: 3,
        server_version: sunshine_gen_7_431(),
    });

    payloader
        .push_frame(0, None, FrameType::PFrame, &data[VideoFrameHeader::SIZE..])
        .unwrap();

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

            let rtp_header = RtpVideoHeader {
                header: 0x80 | VIDEO_FLAG_EXTENSION,
                packet_type: 0,
                sequence_number,
                timestamp: 0,
                ssrc: 0,
                reserved: [0; _],
            };
            let video_header = VideoHeader {
                stream_packet_index: (sequence_number << 8) as u32,
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
            };
            let expected_packet = construct_packet(
                rtp_header.clone(),
                video_header.clone(),
                shards[block as usize * data_shards_total as usize + shard_index as usize],
            );

            info!(?rtp_header, ?video_header, "comparing packet");
            assert_eq!(
                payloader.poll_packet().as_deref(),
                Some(expected_packet.as_slice())
            );

            sequence_number += 1;
        }
    }
}
