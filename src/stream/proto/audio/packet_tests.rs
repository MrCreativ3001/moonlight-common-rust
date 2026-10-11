use crate::stream::proto::audio::packet::{
    AudioFecHeader, RTP_PAYLOAD_TYPE_AUDIO, RTP_PAYLOAD_TYPE_AUDIO_FEC, RtpAudioHeader,
};

#[test]
fn test_audio_rtp_header_serialization() {
    let assert_eq_header = |deserialized: RtpAudioHeader,
                            serialized: [u8; RtpAudioHeader::SIZE]| {
        let mut buffer = [0; _];
        deserialized.serialize(&mut buffer);

        assert_eq!(buffer, serialized);

        assert_eq!(RtpAudioHeader::deserialize(&buffer), deserialized);
    };

    assert_eq_header(
        RtpAudioHeader {
            header: 128,
            packet_type: RTP_PAYLOAD_TYPE_AUDIO,
            sequence_number: 1,
            timestamp: 2,
            ssrc: 0,
        },
        [128, RTP_PAYLOAD_TYPE_AUDIO, 0, 1, 0, 0, 0, 2, 0, 0, 0, 0],
    );
    assert_eq_header(
        RtpAudioHeader {
            header: 128,
            packet_type: RTP_PAYLOAD_TYPE_AUDIO_FEC,
            sequence_number: 1283,
            timestamp: 33816835,
            ssrc: 1,
        },
        [
            128,
            RTP_PAYLOAD_TYPE_AUDIO_FEC,
            5,
            3,
            2,
            4,
            1,
            3,
            0,
            0,
            0,
            1,
        ],
    );
}

#[test]
fn test_audio_fec_header_serialization() {
    let assert_eq_header = |deserialized: AudioFecHeader,
                            serialized: [u8; AudioFecHeader::SIZE]| {
        let mut buffer = [0; _];
        deserialized.serialize(&mut buffer);

        assert_eq!(buffer, serialized);

        assert_eq!(AudioFecHeader::deserialize(&buffer), deserialized);
    };

    assert_eq_header(
        AudioFecHeader {
            fec_shard_index: 1,
            payload_type: 97,
            base_sequence_number: 2,
            base_timestamp: 3,
            ssrc: 0,
        },
        [1, 97, 0, 2, 0, 0, 0, 3, 0, 0, 0, 0],
    );

    assert_eq_header(
        AudioFecHeader {
            fec_shard_index: 1,
            payload_type: 97,
            base_sequence_number: 1283,
            base_timestamp: 33816835,
            ssrc: 1,
        },
        [1, 97, 5, 3, 2, 4, 1, 3, 0, 0, 0, 1],
    );
}
