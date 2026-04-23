use crate::stream::proto::microphone::foundation::packet::{
    FOUNDATION_MIC_HEADER_FLAGS, FOUNDATION_MIC_MAGIC, FOUNDATION_MIC_PACKET_TYPE_OPUS,
    FoundationMicHeader,
};

#[test]
fn foundation_mic_header_serialization() {
    let assert_eq_header =
        |deserialized: FoundationMicHeader, serialized: [u8; FoundationMicHeader::SIZE]| {
            let mut buffer = [0; FoundationMicHeader::SIZE];
            deserialized.serialize(&mut buffer);

            assert_eq!(buffer, serialized);

            assert_eq!(FoundationMicHeader::deserialize(&buffer), deserialized);
        };

    // Simple case (small numbers, easy to eyeball LE encoding)
    assert_eq_header(
        FoundationMicHeader {
            flags: FOUNDATION_MIC_HEADER_FLAGS,
            packet_type: FOUNDATION_MIC_PACKET_TYPE_OPUS,
            sequence_number: 1,
            timestamp: 2,
            ssrc: 0,
        },
        [
            0x00, // flags
            0x61, // packet_type
            0x01, 0x00, // sequence_number (LE)
            0x02, 0x00, 0x00, 0x00, // timestamp (LE)
            0x00, 0x00, 0x00, 0x00, // ssrc (LE)
        ],
    );

    // More complex values to verify byte ordering
    assert_eq_header(
        FoundationMicHeader {
            flags: 0xAB,
            packet_type: FOUNDATION_MIC_PACKET_TYPE_OPUS,
            sequence_number: 0x1234,
            timestamp: 0x01020304,
            ssrc: FOUNDATION_MIC_MAGIC,
        },
        [
            0xAB, // flags
            0x61, // packet_type
            0x34, 0x12, // sequence_number (LE)
            0x04, 0x03, 0x02, 0x01, // timestamp (LE)
            0x78, 0x56, 0x34, 0x12, // ssrc (LE, 0x12345678)
        ],
    );
}

#[test]
fn payloader() {
    todo!()
}
