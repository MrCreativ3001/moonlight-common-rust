use crate::stream::proto::video::packet::{
    FrameType, RtpVideoHeader, VIDEO_FLAG_EXTENSION, VideoFecInfo, VideoFrameHeader, VideoHeader,
    VideoHeaderExtraFlags, VideoHeaderFlags, VideoMultiFecBlocks, fec_percentage_to_parity_shards,
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
            extra_flags: VideoHeaderExtraFlags::LTR_FRAME,
            multi_fec_flags: 10,
            multi_fec_blocks: VideoMultiFecBlocks {
                last_block_index: 1,
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
        [0, 0, 0, 0, 1, 0, 0, 0, 5, 1, 10, 96, 51, 16, 128, 0],
    );

    assert_eq_header(
        VideoHeader {
            stream_packet_index: 104843,
            frame_index: 120,
            flags: VideoHeaderFlags::END_OF_FILE | VideoHeaderFlags::CONTAINS_VIDEO_DATA,
            extra_flags: VideoHeaderExtraFlags::empty(),
            multi_fec_flags: 0,
            multi_fec_blocks: VideoMultiFecBlocks {
                last_block_index: 0,
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
            host_processing_latency: 0,
            frame_type: FrameType::PFrame,
            last_payload_len: 1234,
            reserved: [0, 0],
        },
        [1, 0, 0, 1, 210, 4, 0, 0],
    );

    assert_eq_frame_header(
        VideoFrameHeader {
            header_type: 1,
            host_processing_latency: 0x1234,
            frame_type: FrameType::Idr,
            last_payload_len: 4321,
            reserved: [255, 254],
        },
        [1, 0x34, 0x12, 2, 225, 16, 255, 254],
    );
}

pub(crate) fn construct_packet(
    rtp_header: RtpVideoHeader,
    video_header: VideoHeader,
    data: &[u8],
) -> Vec<u8> {
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
