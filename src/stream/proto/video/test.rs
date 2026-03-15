use std::array;

use crate::stream::{
    proto::video::{
        depayloader::{
            VideoDepayloader, VideoDepayloaderConfig, VideoFrame, create_video_reed_solomon,
        },
        nal::h264,
        packet::{
            FrameType, RtpVideoHeader, VIDEO_FLAG_EXTENSION, VideoFecInfo, VideoFrameHeader,
            VideoHeader, VideoHeaderFlags, VideoMultiFecBlocks, fec_percentage_from,
            fec_percentage_to_parity_shards,
        },
        payloader::{VideoPayloader, VideoPayloaderConfig, VideoPayloaderFecConfig},
    },
    video::{BufferType, VideoFormat, VideoFrameBuffer},
};

// TODO: test encrypted header serialization

#[test]
fn rtp_header_serialization() {
    let assert_eq_header = |deserialized: RtpVideoHeader,
                            serialized: [u8; RtpVideoHeader::SIZE]| {
        let mut buffer = [0; _];
        deserialized.serialize(&mut buffer);

        assert_eq!(buffer, serialized);

        assert_eq!(RtpVideoHeader::deserialize(&buffer), deserialized);
    };

    assert_eq_header(
        RtpVideoHeader {
            header: 0x80 | VIDEO_FLAG_EXTENSION,
            packet_type: 0,
            sequence_number: 1,
            timestamp: 2,
            ssrc: 3,
            reserved: [1, 2, 3, 4],
        },
        [
            0x80 | VIDEO_FLAG_EXTENSION,
            0,
            0,
            1,
            0,
            0,
            0,
            2,
            0,
            0,
            0,
            3,
            1,
            2,
            3,
            4,
        ],
    );

    assert_eq_header(
        RtpVideoHeader {
            header: VIDEO_FLAG_EXTENSION,
            packet_type: 2,
            sequence_number: 1283,
            timestamp: 33816835,
            ssrc: 5,
            reserved: [4; _],
        },
        [
            VIDEO_FLAG_EXTENSION,
            2,
            5,
            3,
            2,
            4,
            1,
            3,
            0,
            0,
            0,
            5,
            4,
            4,
            4,
            4,
        ],
    );
}

#[test]
fn header_serialization() {
    let assert_eq_header = |deserialized: VideoHeader, serialized: [u8; VideoHeader::SIZE]| {
        let mut buffer = [0; _];
        deserialized.serialize(&mut buffer);

        assert_eq!(buffer, serialized);

        assert_eq!(VideoHeader::deserialize(&buffer), deserialized);
    };

    assert_eq_header(
        VideoHeader {
            stream_packet_index: 0,
            frame_index: 1,
            flags: VideoHeaderFlags::START_OF_FILE | VideoHeaderFlags::CONTAINS_VIDEO_DATA,
            reserved: 5,
            multi_fec_flags: 10,
            multi_fec_blocks: VideoMultiFecBlocks {
                block_index: 1,
                current_block: 2,
                unused: 0,
            },
            fec_info: VideoFecInfo {
                shard_index: 1,
                data_shards_total: 2,
                fec_percentage: 3,
                unused: 3,
            },
        },
        [0, 0, 0, 0, 1, 0, 0, 0, 5, 5, 10, 96, 51, 16, 128, 0],
    );

    assert_eq_header(
        VideoHeader {
            stream_packet_index: 104843,
            frame_index: 120,
            flags: VideoHeaderFlags::END_OF_FILE | VideoHeaderFlags::CONTAINS_VIDEO_DATA,
            reserved: 0,
            multi_fec_flags: 0,
            multi_fec_blocks: VideoMultiFecBlocks {
                block_index: 0,
                current_block: 0,
                unused: 0,
            },
            fec_info: VideoFecInfo {
                shard_index: 1,
                data_shards_total: 20,
                fec_percentage: 20,
                unused: 0,
            },
        },
        [139, 153, 1, 0, 120, 0, 0, 0, 3, 0, 0, 0, 64, 17, 0, 5],
    );
}

#[test]
fn frame_header_serialization() {
    let assert_eq_frame_header =
        |deserialized: VideoFrameHeader, serialized: [u8; VideoFrameHeader::SIZE]| {
            let mut buffer = [0; VideoFrameHeader::SIZE];
            deserialized.serialize(&mut buffer);

            assert_eq!(buffer, serialized);

            assert_eq!(VideoFrameHeader::deserialize(&buffer), deserialized);
        };

    assert_eq_frame_header(
        VideoFrameHeader {
            header_type: 1,
            unknown: [0, 0],
            frame_type: FrameType::PFrame,
            last_payload_len: 1234,
            unknown2: [0, 0],
        },
        [1, 0, 0, 1, 210, 4, 0, 0],
    );

    assert_eq_frame_header(
        VideoFrameHeader {
            header_type: 1,
            unknown: [1, 2],
            frame_type: FrameType::Idr,
            last_payload_len: 4321,
            unknown2: [255, 254],
        },
        [1, 1, 2, 2, 225, 16, 255, 254],
    );
}

fn construct_packet(rtp_header: RtpVideoHeader, video_header: VideoHeader, data: &[u8]) -> Vec<u8> {
    let mut buffer = vec![0; RtpVideoHeader::SIZE + VideoHeader::SIZE + data.len()];

    rtp_header.serialize(buffer[0..RtpVideoHeader::SIZE].as_mut_array().unwrap());
    video_header.serialize(
        buffer[RtpVideoHeader::SIZE..(RtpVideoHeader::SIZE + VideoHeader::SIZE)]
            .as_mut_array()
            .unwrap(),
    );
    buffer[(RtpVideoHeader::SIZE + VideoHeader::SIZE)..].copy_from_slice(data);

    buffer
}

#[test]
fn fec_percentage() {
    assert_eq!(fec_percentage_to_parity_shards(10, 20), 2);
    // Note: it rounds up
    assert_eq!(fec_percentage_to_parity_shards(9, 20), 2);

    assert_eq!(fec_percentage_to_parity_shards(20, 50), 10);
}

#[test]
fn payloader_no_fec() {
    let mut data: [u8; 128 + 512] = array::from_fn(|i| (i % u8::MAX as usize) as u8);
    // Copy the frame header
    let frame_header = VideoFrameHeader {
        header_type: 0x01,
        frame_type: FrameType::PFrame,
        unknown: [0; _],
        // The last payload should have 8 bytes (VideoFrameHeader::SIZE) because it's always appended at the beginning
        last_payload_len: VideoFrameHeader::SIZE as u16,
        unknown2: [0; _],
    };
    frame_header.serialize(data[0..VideoFrameHeader::SIZE].as_mut_array().unwrap());
    // Add zero padding
    data[(VideoFrameHeader::SIZE + 512)..].fill(0);

    let data_shards_total = 5;

    let mut payloader = VideoPayloader::new(VideoPayloaderConfig {
        fec: None,
        packet_size: 128 + RtpVideoHeader::SIZE + VideoHeader::SIZE,
    });

    payloader
        .push_frame(
            0,
            FrameType::PFrame,
            &data[VideoFrameHeader::SIZE..(VideoFrameHeader::SIZE + 512)],
        )
        .unwrap();

    assert_eq!(
        payloader.poll_packet(),
        Ok(Some(
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
                    frame_index: 0,
                    flags: VideoHeaderFlags::CONTAINS_VIDEO_DATA | VideoHeaderFlags::START_OF_FILE,
                    reserved: 0,
                    multi_fec_flags: 0x10,
                    multi_fec_blocks: VideoMultiFecBlocks {
                        block_index: 0,
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
        )),
    );
    assert_eq!(
        payloader.poll_packet(),
        Ok(Some(
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
                    frame_index: 0,
                    flags: VideoHeaderFlags::CONTAINS_VIDEO_DATA,
                    reserved: 0,
                    multi_fec_flags: 0x10,
                    multi_fec_blocks: VideoMultiFecBlocks {
                        block_index: 0,
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
        )),
    );
    assert_eq!(
        payloader.poll_packet(),
        Ok(Some(
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
                    frame_index: 0,
                    flags: VideoHeaderFlags::CONTAINS_VIDEO_DATA,
                    reserved: 0,
                    multi_fec_flags: 0x10,
                    multi_fec_blocks: VideoMultiFecBlocks {
                        block_index: 0,
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
        )),
    );
    assert_eq!(
        payloader.poll_packet(),
        Ok(Some(
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
                    frame_index: 0,
                    flags: VideoHeaderFlags::CONTAINS_VIDEO_DATA,
                    reserved: 0,
                    multi_fec_flags: 0x10,
                    multi_fec_blocks: VideoMultiFecBlocks {
                        block_index: 0,
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
        )),
    );
    assert_eq!(
        payloader.poll_packet(),
        Ok(Some(
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
                    frame_index: 0,
                    flags: VideoHeaderFlags::CONTAINS_VIDEO_DATA | VideoHeaderFlags::END_OF_FILE,
                    reserved: 0,
                    multi_fec_flags: 0x10,
                    multi_fec_blocks: VideoMultiFecBlocks {
                        block_index: 0,
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
        )),
    );
    assert_eq!(Ok(None), payloader.poll_packet());
}

fn generate_frame_payload(frame: &[u8], packet_size: usize) -> Vec<u8> {
    let full_payload_len = VideoFrameHeader::SIZE + frame.len();
    let padded_len = full_payload_len.div_ceil(packet_size) * packet_size;
    let mut data = vec![0; padded_len];

    let last_payload_len = if full_payload_len.is_multiple_of(frame.len()) {
        frame.len()
    } else {
        full_payload_len % frame.len()
    };

    // Copy the frame header
    let frame_header = VideoFrameHeader {
        header_type: 0x01,
        frame_type: FrameType::PFrame,
        unknown: [0; _],
        last_payload_len: last_payload_len as u16,
        unknown2: [0; _],
    };
    frame_header.serialize(data[0..VideoFrameHeader::SIZE].as_mut_array().unwrap());

    // Copy Frame
    data[VideoFrameHeader::SIZE..full_payload_len].copy_from_slice(frame);

    // Add zero padding
    data[full_payload_len..].fill(0);

    data
}

#[test]
fn payloader() {
    let packet_size = 128 + RtpVideoHeader::SIZE + VideoHeader::SIZE;

    let frame: [u8; 512] = array::from_fn(|i| (i % u8::MAX as usize) as u8);
    let full_payload = generate_frame_payload(&frame, packet_size);

    let data_shards_total = 5u32;
    let parity_shard_count = 2;
    let fec_percentage = fec_percentage_from(data_shards_total as usize, parity_shard_count) as u32;

    let data_shards = full_payload.chunks(128).collect::<Vec<_>>();
    let mut fec_data = vec![vec![0; 128]; 2];

    let reed_solomon = create_video_reed_solomon(data_shards_total as usize, parity_shard_count);
    reed_solomon
        .encode_sep(&data_shards, &mut fec_data)
        .unwrap();

    let mut payloader = VideoPayloader::new(VideoPayloaderConfig {
        fec: Some(VideoPayloaderFecConfig {
            fec_percentage: 0,
            min_required_fec_packets: parity_shard_count,
        }),
        packet_size,
    });

    payloader.push_frame(0, FrameType::PFrame, &frame).unwrap();

    assert_eq!(
        payloader.poll_packet(),
        Ok(Some(
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
                    frame_index: 0,
                    flags: VideoHeaderFlags::CONTAINS_VIDEO_DATA | VideoHeaderFlags::START_OF_FILE,
                    reserved: 0,
                    multi_fec_flags: 0x10,
                    multi_fec_blocks: VideoMultiFecBlocks {
                        block_index: 0,
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
        )),
    );
    assert_eq!(
        payloader.poll_packet(),
        Ok(Some(
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
                    frame_index: 0,
                    flags: VideoHeaderFlags::CONTAINS_VIDEO_DATA,
                    reserved: 0,
                    multi_fec_flags: 0x10,
                    multi_fec_blocks: VideoMultiFecBlocks {
                        block_index: 0,
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
        )),
    );
    assert_eq!(
        payloader.poll_packet(),
        Ok(Some(
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
                    frame_index: 0,
                    flags: VideoHeaderFlags::CONTAINS_VIDEO_DATA,
                    reserved: 0,
                    multi_fec_flags: 0x10,
                    multi_fec_blocks: VideoMultiFecBlocks {
                        block_index: 0,
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
        )),
    );
    assert_eq!(
        payloader.poll_packet(),
        Ok(Some(
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
                    frame_index: 0,
                    flags: VideoHeaderFlags::CONTAINS_VIDEO_DATA,
                    reserved: 0,
                    multi_fec_flags: 0x10,
                    multi_fec_blocks: VideoMultiFecBlocks {
                        block_index: 0,
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
        )),
    );
    assert_eq!(
        payloader.poll_packet(),
        Ok(Some(
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
                    frame_index: 0,
                    flags: VideoHeaderFlags::CONTAINS_VIDEO_DATA | VideoHeaderFlags::END_OF_FILE,
                    reserved: 0,
                    multi_fec_flags: 0x10,
                    multi_fec_blocks: VideoMultiFecBlocks {
                        block_index: 0,
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
        )),
    );
    assert_eq!(
        payloader.poll_packet(),
        Ok(Some(
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
                    frame_index: 0,
                    flags: VideoHeaderFlags::empty(),
                    reserved: 0,
                    multi_fec_flags: 0x10,
                    multi_fec_blocks: VideoMultiFecBlocks {
                        block_index: 0,
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
        )),
    );
    assert_eq!(
        payloader.poll_packet(),
        Ok(Some(
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
                    frame_index: 0,
                    flags: VideoHeaderFlags::empty(),
                    reserved: 0,
                    multi_fec_flags: 0x10,
                    multi_fec_blocks: VideoMultiFecBlocks {
                        block_index: 0,
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
        )),
    );
    assert_eq!(Ok(None), payloader.poll_packet());
}

#[test]
fn depayloader_simple_noparse() {
    let mut depayloader = VideoDepayloader::new(VideoDepayloaderConfig {
        packet_size: VideoHeader::SIZE + 10,
        format: VideoFormat::Av1Main8,
    });

    let expected1 = vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9];
    let expected2 = vec![9, 8, 7, 6, 5, 4, 3, 2, 1, 0];
    let expected3 = vec![0, 1, 2, 3, 4, 9, 7, 8, 6, 5];

    depayloader
        .handle_packet(&construct_packet(
            RtpVideoHeader {
                header: 0x80 | VIDEO_FLAG_EXTENSION,
                packet_type: 0,
                sequence_number: 0,
                timestamp: 0,
                ssrc: 0,
                reserved: [0; 4],
            },
            VideoHeader {
                stream_packet_index: 0,
                frame_index: 1,
                flags: VideoHeaderFlags::START_OF_FILE | VideoHeaderFlags::CONTAINS_VIDEO_DATA,
                reserved: 0,
                multi_fec_flags: 0,
                multi_fec_blocks: VideoMultiFecBlocks {
                    block_index: 0,
                    current_block: 0,
                    unused: 0,
                },
                fec_info: VideoFecInfo {
                    data_shards_total: 3,
                    shard_index: 0,
                    fec_percentage: 0,
                    unused: 0,
                },
            },
            &expected1,
        ))
        .unwrap();
    assert_eq!(depayloader.poll_frame(), Ok(None));

    depayloader
        .handle_packet(&construct_packet(
            RtpVideoHeader {
                header: 0x80 | VIDEO_FLAG_EXTENSION,
                packet_type: 0,
                sequence_number: 1,
                timestamp: 0,
                ssrc: 0,
                reserved: [0; 4],
            },
            VideoHeader {
                stream_packet_index: 0,
                frame_index: 1,
                flags: VideoHeaderFlags::START_OF_FILE | VideoHeaderFlags::CONTAINS_VIDEO_DATA,
                reserved: 0,
                multi_fec_flags: 0,
                multi_fec_blocks: VideoMultiFecBlocks {
                    block_index: 0,
                    current_block: 0,
                    unused: 0,
                },
                fec_info: VideoFecInfo {
                    data_shards_total: 3,
                    shard_index: 1,
                    fec_percentage: 0,
                    unused: 0,
                },
            },
            &expected2,
        ))
        .unwrap();
    assert_eq!(depayloader.poll_frame(), Ok(None));

    depayloader
        .handle_packet(&construct_packet(
            RtpVideoHeader {
                header: 0x80 | VIDEO_FLAG_EXTENSION,
                packet_type: 0,
                sequence_number: 2,
                timestamp: 0,
                ssrc: 0,
                reserved: [0; 4],
            },
            VideoHeader {
                stream_packet_index: 0,
                frame_index: 1,
                flags: VideoHeaderFlags::END_OF_FILE | VideoHeaderFlags::CONTAINS_VIDEO_DATA,
                reserved: 0,
                multi_fec_flags: 0,
                multi_fec_blocks: VideoMultiFecBlocks {
                    block_index: 0,
                    current_block: 0,
                    unused: 0,
                },
                fec_info: VideoFecInfo {
                    data_shards_total: 3,
                    shard_index: 2,
                    fec_percentage: 0,
                    unused: 0,
                },
            },
            &expected3,
        ))
        .unwrap();
    assert_eq!(
        depayloader.poll_frame(),
        // All data should be aligned to packets
        Ok(Some(VideoFrame {
            frame_number: 1,
            timestamp: 0,
            buffers: vec![
                VideoFrameBuffer {
                    buffer_type: BufferType::PicData,
                    data: expected1.clone(),
                },
                VideoFrameBuffer {
                    buffer_type: BufferType::PicData,
                    data: expected2.clone()
                },
                VideoFrameBuffer {
                    buffer_type: BufferType::PicData,
                    data: expected3.clone()
                }
            ],
        }))
    );
    assert_eq!(depayloader.poll_frame(), Ok(None));
}

#[test]
fn depayloader_simple_noparse_multiframe() {
    let mut depayloader = VideoDepayloader::new(VideoDepayloaderConfig {
        packet_size: VideoHeader::SIZE + 10,
        format: VideoFormat::Av1Main8,
    });

    let expected1 = vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9];
    let expected2 = vec![9, 8, 7, 6, 5, 4, 3, 2, 1, 0];
    let expected3 = vec![0, 1, 2, 3, 4, 9, 7, 8, 6, 5];

    // -- Frame 1
    depayloader
        .handle_packet(&construct_packet(
            RtpVideoHeader {
                header: 0x80 | VIDEO_FLAG_EXTENSION,
                packet_type: 0,
                sequence_number: 0,
                timestamp: 0,
                ssrc: 0,
                reserved: [0; 4],
            },
            VideoHeader {
                stream_packet_index: 0,
                frame_index: 1,
                flags: VideoHeaderFlags::START_OF_FILE | VideoHeaderFlags::CONTAINS_VIDEO_DATA,
                reserved: 0,
                multi_fec_flags: 0,
                multi_fec_blocks: VideoMultiFecBlocks {
                    block_index: 0,
                    current_block: 0,
                    unused: 0,
                },
                fec_info: VideoFecInfo {
                    data_shards_total: 3,
                    shard_index: 0,
                    fec_percentage: 0,
                    unused: 0,
                },
            },
            &expected1,
        ))
        .unwrap();
    assert_eq!(depayloader.poll_frame(), Ok(None));

    depayloader
        .handle_packet(&construct_packet(
            RtpVideoHeader {
                header: 0x80 | VIDEO_FLAG_EXTENSION,
                packet_type: 0,
                sequence_number: 1,
                timestamp: 0,
                ssrc: 0,
                reserved: [0; 4],
            },
            VideoHeader {
                stream_packet_index: 0,
                frame_index: 1,
                flags: VideoHeaderFlags::CONTAINS_VIDEO_DATA,
                reserved: 0,
                multi_fec_flags: 0,
                multi_fec_blocks: VideoMultiFecBlocks {
                    block_index: 0,
                    current_block: 0,
                    unused: 0,
                },
                fec_info: VideoFecInfo {
                    data_shards_total: 3,
                    shard_index: 1,
                    fec_percentage: 0,
                    unused: 0,
                },
            },
            &expected2,
        ))
        .unwrap();
    assert_eq!(depayloader.poll_frame(), Ok(None));

    depayloader
        .handle_packet(&construct_packet(
            RtpVideoHeader {
                header: 0x80 | VIDEO_FLAG_EXTENSION,
                packet_type: 0,
                sequence_number: 2,
                timestamp: 0,
                ssrc: 0,
                reserved: [0; 4],
            },
            VideoHeader {
                stream_packet_index: 0,
                frame_index: 1,
                flags: VideoHeaderFlags::END_OF_FILE | VideoHeaderFlags::CONTAINS_VIDEO_DATA,
                reserved: 0,
                multi_fec_flags: 0,
                multi_fec_blocks: VideoMultiFecBlocks {
                    block_index: 0,
                    current_block: 0,
                    unused: 0,
                },
                fec_info: VideoFecInfo {
                    data_shards_total: 3,
                    shard_index: 2,
                    fec_percentage: 0,
                    unused: 0,
                },
            },
            &expected3,
        ))
        .unwrap();
    assert_eq!(
        depayloader.poll_frame(),
        // All data should be aligned to packets
        Ok(Some(VideoFrame {
            frame_number: 1,
            timestamp: 0,
            buffers: vec![
                VideoFrameBuffer {
                    buffer_type: BufferType::PicData,
                    data: expected1.clone(),
                },
                VideoFrameBuffer {
                    buffer_type: BufferType::PicData,
                    data: expected2.clone()
                },
                VideoFrameBuffer {
                    buffer_type: BufferType::PicData,
                    data: expected3.clone()
                }
            ],
        }))
    );
    assert_eq!(depayloader.poll_frame(), Ok(None));

    // -- Frame 2
    depayloader
        .handle_packet(&construct_packet(
            RtpVideoHeader {
                header: 0x80 | VIDEO_FLAG_EXTENSION,
                packet_type: 0,
                sequence_number: 3,
                timestamp: 100,
                ssrc: 0,
                reserved: [0; 4],
            },
            VideoHeader {
                stream_packet_index: 0,
                frame_index: 2,
                flags: VideoHeaderFlags::START_OF_FILE | VideoHeaderFlags::CONTAINS_VIDEO_DATA,
                reserved: 0,
                multi_fec_flags: 0,
                multi_fec_blocks: VideoMultiFecBlocks {
                    block_index: 0,
                    current_block: 0,
                    unused: 0,
                },
                fec_info: VideoFecInfo {
                    data_shards_total: 3,
                    shard_index: 0,
                    fec_percentage: 0,
                    unused: 0,
                },
            },
            &expected1,
        ))
        .unwrap();
    assert_eq!(depayloader.poll_frame(), Ok(None));

    depayloader
        .handle_packet(&construct_packet(
            RtpVideoHeader {
                header: 0x80 | VIDEO_FLAG_EXTENSION,
                packet_type: 0,
                sequence_number: 4,
                timestamp: 100,
                ssrc: 0,
                reserved: [0; 4],
            },
            VideoHeader {
                stream_packet_index: 0,
                frame_index: 2,
                flags: VideoHeaderFlags::CONTAINS_VIDEO_DATA,
                reserved: 0,
                multi_fec_flags: 0,
                multi_fec_blocks: VideoMultiFecBlocks {
                    block_index: 0,
                    current_block: 0,
                    unused: 0,
                },
                fec_info: VideoFecInfo {
                    data_shards_total: 3,
                    shard_index: 1,
                    fec_percentage: 0,
                    unused: 0,
                },
            },
            &expected2,
        ))
        .unwrap();
    assert_eq!(depayloader.poll_frame(), Ok(None));

    depayloader
        .handle_packet(&construct_packet(
            RtpVideoHeader {
                header: 0x80 | VIDEO_FLAG_EXTENSION,
                packet_type: 0,
                sequence_number: 5,
                timestamp: 100,
                ssrc: 0,
                reserved: [0; 4],
            },
            VideoHeader {
                stream_packet_index: 0,
                frame_index: 2,
                flags: VideoHeaderFlags::END_OF_FILE | VideoHeaderFlags::CONTAINS_VIDEO_DATA,
                reserved: 0,
                multi_fec_flags: 0,
                multi_fec_blocks: VideoMultiFecBlocks {
                    block_index: 0,
                    current_block: 0,
                    unused: 0,
                },
                fec_info: VideoFecInfo {
                    data_shards_total: 3,
                    shard_index: 2,
                    fec_percentage: 0,
                    unused: 0,
                },
            },
            &expected3,
        ))
        .unwrap();
    assert_eq!(
        depayloader.poll_frame(),
        // All data should be aligned to packets
        Ok(Some(VideoFrame {
            frame_number: 2,
            timestamp: 100,
            buffers: vec![
                VideoFrameBuffer {
                    buffer_type: BufferType::PicData,
                    data: expected1.clone(),
                },
                VideoFrameBuffer {
                    buffer_type: BufferType::PicData,
                    data: expected2.clone()
                },
                VideoFrameBuffer {
                    buffer_type: BufferType::PicData,
                    data: expected3.clone()
                }
            ],
        }))
    );
    assert_eq!(depayloader.poll_frame(), Ok(None));
}

#[test]
fn depayloader_simple_h264() {
    let mut depayloader = VideoDepayloader::new(VideoDepayloaderConfig {
        packet_size: VideoHeader::SIZE + 20,
        format: VideoFormat::H264,
    });

    let nal_header_sps = h264::NalHeader {
        forbidden_zero_bit: false,
        nal_ref_idc: 0,
        nal_unit_type: h264::NalUnitType::Sps,
    };
    let expected1 = vec![
        0,
        0,
        0,
        1,
        nal_header_sps.serialize()[0],
        4,
        5,
        6,
        7,
        8,
        9,
        10,
        11,
        12,
        13,
        14,
        15,
        16,
        17,
        18,
    ];

    let nal_header_pps = h264::NalHeader {
        forbidden_zero_bit: false,
        nal_ref_idc: 0,
        nal_unit_type: h264::NalUnitType::Pps,
    };
    let expected2 = vec![
        0,
        0,
        0,
        1,
        nal_header_pps.serialize()[0],
        4,
        5,
        6,
        7,
        8,
        9,
        9,
        8,
        7,
        6,
        5,
        4,
        3,
        2,
        1,
    ];

    let nal_header_pic_data = h264::NalHeader {
        forbidden_zero_bit: false,
        nal_ref_idc: 0,
        nal_unit_type: h264::NalUnitType::CodedSliceNonIDR,
    };
    let expected3 = vec![
        0,
        0,
        0,
        1,
        nal_header_pic_data.serialize()[0],
        7,
        6,
        5,
        4,
        3,
        2,
        1,
        0,
        0,
        2,
        2,
        3,
        4,
        5,
        6,
    ];

    depayloader
        .handle_packet(&construct_packet(
            RtpVideoHeader {
                header: 0x80 | VIDEO_FLAG_EXTENSION,
                packet_type: 0,
                sequence_number: 0,
                timestamp: 0,
                ssrc: 0,
                reserved: [0; 4],
            },
            VideoHeader {
                stream_packet_index: 0,
                frame_index: 1,
                flags: VideoHeaderFlags::START_OF_FILE | VideoHeaderFlags::CONTAINS_VIDEO_DATA,
                reserved: 0,
                multi_fec_flags: 0,
                multi_fec_blocks: VideoMultiFecBlocks {
                    block_index: 0,
                    current_block: 0,
                    unused: 0,
                },
                fec_info: VideoFecInfo {
                    data_shards_total: 3,
                    shard_index: 0,
                    fec_percentage: 0,
                    unused: 0,
                },
            },
            &expected1,
        ))
        .unwrap();
    assert_eq!(depayloader.poll_frame(), Ok(None));

    depayloader
        .handle_packet(&construct_packet(
            RtpVideoHeader {
                header: 0x80 | VIDEO_FLAG_EXTENSION,
                packet_type: 0,
                sequence_number: 1,
                timestamp: 0,
                ssrc: 0,
                reserved: [0; 4],
            },
            VideoHeader {
                stream_packet_index: 0,
                frame_index: 1,
                flags: VideoHeaderFlags::CONTAINS_VIDEO_DATA,
                reserved: 0,
                multi_fec_flags: 0,
                multi_fec_blocks: VideoMultiFecBlocks {
                    block_index: 0,
                    current_block: 0,
                    unused: 0,
                },
                fec_info: VideoFecInfo {
                    data_shards_total: 3,
                    shard_index: 1,
                    fec_percentage: 0,
                    unused: 0,
                },
            },
            &expected2,
        ))
        .unwrap();
    assert_eq!(depayloader.poll_frame(), Ok(None));

    depayloader
        .handle_packet(&construct_packet(
            RtpVideoHeader {
                header: 0x80 | VIDEO_FLAG_EXTENSION,
                packet_type: 0,
                sequence_number: 2,
                timestamp: 0,
                ssrc: 0,
                reserved: [0; 4],
            },
            VideoHeader {
                stream_packet_index: 0,
                frame_index: 1,
                flags: VideoHeaderFlags::END_OF_FILE | VideoHeaderFlags::CONTAINS_VIDEO_DATA,
                reserved: 0,
                multi_fec_flags: 0,
                multi_fec_blocks: VideoMultiFecBlocks {
                    block_index: 0,
                    current_block: 0,
                    unused: 0,
                },
                fec_info: VideoFecInfo {
                    data_shards_total: 3,
                    shard_index: 2,
                    fec_percentage: 0,
                    unused: 0,
                },
            },
            &expected3,
        ))
        .unwrap();
    assert_eq!(
        depayloader.poll_frame(),
        Ok(Some(VideoFrame {
            frame_number: 1,
            timestamp: 0,
            buffers: vec![
                VideoFrameBuffer {
                    buffer_type: BufferType::Sps,
                    data: expected1.clone(),
                },
                VideoFrameBuffer {
                    buffer_type: BufferType::Pps,
                    data: expected2.clone()
                },
                VideoFrameBuffer {
                    buffer_type: BufferType::PicData,
                    data: expected3.clone()
                }
            ],
        }))
    );
    assert_eq!(depayloader.poll_frame(), Ok(None));
}

#[test]
fn depayloader_complex_h264() {
    let mut depayloader = VideoDepayloader::new(VideoDepayloaderConfig {
        packet_size: VideoHeader::SIZE + 10,
        format: VideoFormat::H264,
    });

    let nal_header_sps = h264::NalHeader {
        forbidden_zero_bit: false,
        nal_ref_idc: 0,
        nal_unit_type: h264::NalUnitType::Sps,
    };

    let nal_header_pps = h264::NalHeader {
        forbidden_zero_bit: false,
        nal_ref_idc: 0,
        nal_unit_type: h264::NalUnitType::Pps,
    };

    let nal_header_pic_data = h264::NalHeader {
        forbidden_zero_bit: false,
        nal_ref_idc: 0,
        nal_unit_type: h264::NalUnitType::CodedSliceNonIDR,
    };

    let data1 = [0, 0, 0, 1, nal_header_sps.serialize()[0], 4, 5, 6, 0, 0];
    let data2 = [0, 1, nal_header_pps.serialize()[0], 4, 3, 2, 0, 0, 0, 1];
    let data3 = [
        nal_header_pic_data.serialize()[0],
        5,
        6,
        7,
        8,
        7,
        6,
        5,
        4,
        3,
    ];

    let expected1 = vec![0, 0, 0, 1, nal_header_sps.serialize()[0], 4, 5, 6];
    let expected2 = vec![0, 0, 0, 1, nal_header_pps.serialize()[0], 4, 3, 2];
    let expected3 = vec![
        0,
        0,
        0,
        1,
        nal_header_pic_data.serialize()[0],
        5,
        6,
        7,
        8,
        7,
        6,
        5,
        4,
        3,
    ];

    depayloader
        .handle_packet(&construct_packet(
            RtpVideoHeader {
                header: 0x80 | VIDEO_FLAG_EXTENSION,
                packet_type: 0,
                sequence_number: 0,
                timestamp: 0,
                ssrc: 0,
                reserved: [0; 4],
            },
            VideoHeader {
                stream_packet_index: 0,
                frame_index: 1,
                flags: VideoHeaderFlags::START_OF_FILE | VideoHeaderFlags::CONTAINS_VIDEO_DATA,
                reserved: 0,
                multi_fec_flags: 0,
                multi_fec_blocks: VideoMultiFecBlocks {
                    block_index: 0,
                    current_block: 0,
                    unused: 0,
                },
                fec_info: VideoFecInfo {
                    data_shards_total: 3,
                    shard_index: 0,
                    fec_percentage: 0,
                    unused: 0,
                },
            },
            &data1,
        ))
        .unwrap();
    assert_eq!(depayloader.poll_frame(), Ok(None));

    depayloader
        .handle_packet(&construct_packet(
            RtpVideoHeader {
                header: 0x80 | VIDEO_FLAG_EXTENSION,
                packet_type: 0,
                sequence_number: 1,
                timestamp: 0,
                ssrc: 0,
                reserved: [0; 4],
            },
            VideoHeader {
                stream_packet_index: 0,
                frame_index: 1,
                flags: VideoHeaderFlags::CONTAINS_VIDEO_DATA,
                reserved: 0,
                multi_fec_flags: 0,
                multi_fec_blocks: VideoMultiFecBlocks {
                    block_index: 0,
                    current_block: 0,
                    unused: 0,
                },
                fec_info: VideoFecInfo {
                    data_shards_total: 3,
                    shard_index: 1,
                    fec_percentage: 0,
                    unused: 0,
                },
            },
            &data2,
        ))
        .unwrap();
    assert_eq!(depayloader.poll_frame(), Ok(None));

    depayloader
        .handle_packet(&construct_packet(
            RtpVideoHeader {
                header: 0x80 | VIDEO_FLAG_EXTENSION,
                packet_type: 0,
                sequence_number: 2,
                timestamp: 0,
                ssrc: 0,
                reserved: [0; 4],
            },
            VideoHeader {
                stream_packet_index: 0,
                frame_index: 1,
                flags: VideoHeaderFlags::END_OF_FILE | VideoHeaderFlags::CONTAINS_VIDEO_DATA,
                reserved: 0,
                multi_fec_flags: 0,
                multi_fec_blocks: VideoMultiFecBlocks {
                    block_index: 0,
                    current_block: 0,
                    unused: 0,
                },
                fec_info: VideoFecInfo {
                    data_shards_total: 3,
                    shard_index: 2,
                    fec_percentage: 0,
                    unused: 0,
                },
            },
            &data3,
        ))
        .unwrap();
    assert_eq!(
        depayloader.poll_frame(),
        Ok(Some(VideoFrame {
            frame_number: 1,
            timestamp: 0,
            buffers: vec![
                VideoFrameBuffer {
                    buffer_type: BufferType::Sps,
                    data: expected1.clone(),
                },
                VideoFrameBuffer {
                    buffer_type: BufferType::Pps,
                    data: expected2.clone()
                },
                VideoFrameBuffer {
                    buffer_type: BufferType::PicData,
                    data: expected3.clone()
                }
            ],
        }))
    );
    assert_eq!(depayloader.poll_frame(), Ok(None));
}
