use std::array;

use crate::stream::{
    audio::AudioSample,
    proto::audio::{
        create_audio_reed_solomon,
        depayloader::{AudioDepayloader, AudioDepayloaderConfig},
        packet::{
            AudioFecHeader, RTP_AUDIO_DATA_SHARDS, RTP_AUDIO_FEC_SHARDS, RTP_AUDIO_HEADER,
            RTP_AUDIO_TOTAL_SHARDS, RTP_PAYLOAD_TYPE_AUDIO, RTP_PAYLOAD_TYPE_AUDIO_FEC,
            RtpAudioHeader,
        },
        payloader::{AudioPayloader, AudioPayloaderConfig},
    },
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

fn construct_data_packet(rtp_header: RtpAudioHeader, data: &[u8]) -> Vec<u8> {
    assert_eq!(rtp_header.packet_type, RTP_PAYLOAD_TYPE_AUDIO);

    let mut buffer = vec![0u8; RtpAudioHeader::SIZE + data.len()];

    rtp_header.serialize(
        buffer[0..RtpAudioHeader::SIZE]
            .as_mut_array::<{ RtpAudioHeader::SIZE }>()
            .unwrap(),
    );
    buffer[RtpAudioHeader::SIZE..].copy_from_slice(data);

    buffer
}

fn construct_fec_packet(
    rtp_header: RtpAudioHeader,
    fec_header: AudioFecHeader,
    data: &[u8],
) -> Vec<u8> {
    assert_eq!(rtp_header.packet_type, RTP_PAYLOAD_TYPE_AUDIO_FEC);

    let mut buffer = vec![0u8; RtpAudioHeader::SIZE + AudioFecHeader::SIZE + data.len()];

    rtp_header.serialize(buffer[0..RtpAudioHeader::SIZE].as_mut_array().unwrap());
    fec_header.serialize(
        buffer[RtpAudioHeader::SIZE..(RtpAudioHeader::SIZE + AudioFecHeader::SIZE)]
            .as_mut_array()
            .unwrap(),
    );
    buffer[(RtpAudioHeader::SIZE + AudioFecHeader::SIZE)..].copy_from_slice(data);

    buffer
}

fn construct_fec_block(
    base_sequence_number: u16,
    base_timestamp: u32,
    timestamp_increase: u32,
    ssrc: u32,
    data: [&[u8]; RTP_AUDIO_DATA_SHARDS],
) -> [Vec<u8>; RTP_AUDIO_TOTAL_SHARDS] {
    let mut shards = array::from_fn(|_| Vec::default());

    // Construct data packets
    let shard_len = data[0].len();
    for (data_shard, data) in data.iter().enumerate() {
        assert_eq!(
            data.len(),
            shard_len,
            "Every packet must have the same length!"
        );

        shards[data_shard] = construct_data_packet(
            RtpAudioHeader {
                header: 128,
                packet_type: RTP_PAYLOAD_TYPE_AUDIO,
                sequence_number: base_sequence_number + data_shard as u16,
                timestamp: base_timestamp + (timestamp_increase * data_shard as u32),
                ssrc,
            },
            data,
        )
    }

    // Create fec shards
    let mut data_shards: [_; RTP_AUDIO_DATA_SHARDS] = array::from_fn(|i| data[i].to_vec());
    let mut fec_shards: [_; RTP_AUDIO_FEC_SHARDS] = array::from_fn(|_| vec![0u8; shard_len]);

    let mut all_shards = Vec::new();
    for data_shard in &mut data_shards {
        all_shards.push(data_shard);
    }
    for fec_shard in &mut fec_shards {
        all_shards.push(fec_shard);
    }

    let fec_encoder = create_audio_reed_solomon();
    fec_encoder.encode(&mut all_shards).unwrap();

    // Construct fec packets
    for (fec_shard, data) in all_shards[RTP_AUDIO_DATA_SHARDS..RTP_AUDIO_TOTAL_SHARDS]
        .iter()
        .enumerate()
    {
        shards[RTP_AUDIO_DATA_SHARDS + fec_shard] = construct_fec_packet(
            RtpAudioHeader {
                header: 128,
                packet_type: RTP_PAYLOAD_TYPE_AUDIO_FEC,
                sequence_number: base_sequence_number
                    + RTP_AUDIO_DATA_SHARDS as u16
                    + fec_shard as u16,
                timestamp: 0,
                ssrc,
            },
            AudioFecHeader {
                fec_shard_index: fec_shard as u8,
                // This just seems to be 97
                payload_type: 97,
                base_sequence_number,
                base_timestamp,
                ssrc,
            },
            data,
        );
    }

    shards
}

#[test]
fn test_audio_construct_fec_block() {
    let first_data = &[0, 1, 2, 3];
    let second_data = &[4, 5, 6, 7];
    let third_data = &[8, 9, 10, 11];
    let fourth_data = &[12, 13, 14, 15];
    // This is the fec data generated by Sunshine for that specific block
    let first_fec = &[136, 137, 138, 139];
    let second_fec = &[36, 37, 38, 39];

    let base_sequence_number = 1;
    let base_timestamp = 2;
    let timestamp_increase = 5;
    let ssrc = 3;

    let block = construct_fec_block(
        base_sequence_number,
        base_timestamp,
        timestamp_increase,
        ssrc,
        [first_data, second_data, third_data, fourth_data],
    );

    assert_eq!(
        block[0],
        construct_data_packet(
            RtpAudioHeader {
                header: 128,
                packet_type: RTP_PAYLOAD_TYPE_AUDIO,
                sequence_number: base_sequence_number,
                timestamp: base_timestamp,
                ssrc,
            },
            first_data,
        )
    );
    assert_eq!(
        block[1],
        construct_data_packet(
            RtpAudioHeader {
                header: 128,
                packet_type: RTP_PAYLOAD_TYPE_AUDIO,
                sequence_number: base_sequence_number + 1,
                timestamp: base_timestamp + timestamp_increase,
                ssrc,
            },
            second_data,
        )
    );
    assert_eq!(
        block[2],
        construct_data_packet(
            RtpAudioHeader {
                header: 128,
                packet_type: RTP_PAYLOAD_TYPE_AUDIO,
                sequence_number: base_sequence_number + 2,
                timestamp: base_timestamp + timestamp_increase * 2,
                ssrc,
            },
            third_data,
        )
    );
    assert_eq!(
        block[3],
        construct_data_packet(
            RtpAudioHeader {
                header: 128,
                packet_type: RTP_PAYLOAD_TYPE_AUDIO,
                sequence_number: base_sequence_number + 3,
                timestamp: base_timestamp + timestamp_increase * 3,
                ssrc,
            },
            fourth_data,
        )
    );
    assert_eq!(
        block[4],
        construct_fec_packet(
            RtpAudioHeader {
                header: 128,
                packet_type: RTP_PAYLOAD_TYPE_AUDIO_FEC,
                sequence_number: base_sequence_number + 4,
                timestamp: 0,
                ssrc,
            },
            AudioFecHeader {
                fec_shard_index: 0,
                payload_type: 97,
                base_sequence_number,
                base_timestamp,
                ssrc
            },
            first_fec,
        )
    );
    assert_eq!(
        block[5],
        construct_fec_packet(
            RtpAudioHeader {
                header: 128,
                packet_type: RTP_PAYLOAD_TYPE_AUDIO_FEC,
                sequence_number: base_sequence_number + 5,
                timestamp: 0,
                ssrc,
            },
            AudioFecHeader {
                fec_shard_index: 1,
                payload_type: 97,
                base_sequence_number,
                base_timestamp,
                ssrc
            },
            second_fec,
        )
    );
}

#[test]
pub fn test_audio_payloader_no_fec() {
    let first_data = &[0, 1, 2, 3];
    let second_data = &[4, 5, 6, 7];
    let third_data = &[8, 9, 10, 11];
    let fourth_data = &[12, 13, 14, 15];

    let mut payloader = AudioPayloader::new(AudioPayloaderConfig {
        fec: false,
        frame_len: 4,
    });

    payloader.push_frame(0, first_data).unwrap();
    assert_eq!(
        payloader.poll_packet(),
        Ok(Some(
            construct_data_packet(
                RtpAudioHeader {
                    header: RTP_AUDIO_HEADER,
                    packet_type: RTP_PAYLOAD_TYPE_AUDIO,
                    sequence_number: 0,
                    timestamp: 0,
                    ssrc: 0,
                },
                first_data,
            )
            .as_slice()
        ))
    );
    assert_eq!(payloader.poll_packet(), Ok(None));

    payloader.push_frame(5, second_data).unwrap();
    assert_eq!(
        payloader.poll_packet(),
        Ok(Some(
            construct_data_packet(
                RtpAudioHeader {
                    header: RTP_AUDIO_HEADER,
                    packet_type: RTP_PAYLOAD_TYPE_AUDIO,
                    sequence_number: 1,
                    timestamp: 5,
                    ssrc: 0,
                },
                second_data,
            )
            .as_slice()
        ))
    );
    assert_eq!(payloader.poll_packet(), Ok(None));

    payloader.push_frame(10, third_data).unwrap();
    assert_eq!(
        payloader.poll_packet(),
        Ok(Some(
            construct_data_packet(
                RtpAudioHeader {
                    header: RTP_AUDIO_HEADER,
                    packet_type: RTP_PAYLOAD_TYPE_AUDIO,
                    sequence_number: 2,
                    timestamp: 10,
                    ssrc: 0,
                },
                third_data,
            )
            .as_slice()
        ))
    );
    assert_eq!(payloader.poll_packet(), Ok(None));

    payloader.push_frame(15, fourth_data).unwrap();
    assert_eq!(
        payloader.poll_packet(),
        Ok(Some(
            construct_data_packet(
                RtpAudioHeader {
                    header: RTP_AUDIO_HEADER,
                    packet_type: RTP_PAYLOAD_TYPE_AUDIO,
                    sequence_number: 3,
                    timestamp: 15,
                    ssrc: 0,
                },
                fourth_data,
            )
            .as_slice()
        ))
    );
    assert_eq!(payloader.poll_packet(), Ok(None));
}

#[test]
pub fn test_audio_payloader() {
    let first_data = &[0, 1, 2, 3];
    let second_data = &[4, 5, 6, 7];
    let third_data = &[8, 9, 10, 11];
    let fourth_data = &[12, 13, 14, 15];
    // This is the fec data generated by Sunshine for that specific block
    let first_fec = &[136, 137, 138, 139];
    let second_fec = &[36, 37, 38, 39];

    let mut payloader = AudioPayloader::new(AudioPayloaderConfig {
        fec: true,
        frame_len: 4,
    });

    println!("First frame");
    payloader.push_frame(0, first_data).unwrap();
    assert_eq!(
        payloader.poll_packet(),
        Ok(Some(
            construct_data_packet(
                RtpAudioHeader {
                    header: RTP_AUDIO_HEADER,
                    packet_type: RTP_PAYLOAD_TYPE_AUDIO,
                    sequence_number: 0,
                    timestamp: 0,
                    ssrc: 0,
                },
                first_data,
            )
            .as_slice()
        ))
    );
    assert_eq!(payloader.poll_packet(), Ok(None));

    println!("Second frame");
    payloader.push_frame(5, second_data).unwrap();
    assert_eq!(
        payloader.poll_packet(),
        Ok(Some(
            construct_data_packet(
                RtpAudioHeader {
                    header: RTP_AUDIO_HEADER,
                    packet_type: RTP_PAYLOAD_TYPE_AUDIO,
                    sequence_number: 1,
                    timestamp: 5,
                    ssrc: 0,
                },
                second_data,
            )
            .as_slice()
        ))
    );
    assert_eq!(payloader.poll_packet(), Ok(None));

    println!("Third frame");
    payloader.push_frame(10, third_data).unwrap();
    assert_eq!(
        payloader.poll_packet(),
        Ok(Some(
            construct_data_packet(
                RtpAudioHeader {
                    header: RTP_AUDIO_HEADER,
                    packet_type: RTP_PAYLOAD_TYPE_AUDIO,
                    sequence_number: 2,
                    timestamp: 10,
                    ssrc: 0,
                },
                third_data,
            )
            .as_slice()
        ))
    );
    assert_eq!(payloader.poll_packet(), Ok(None));

    println!("Fourth frame");
    payloader.push_frame(15, fourth_data).unwrap();
    assert_eq!(
        payloader.poll_packet(),
        Ok(Some(
            construct_data_packet(
                RtpAudioHeader {
                    header: RTP_AUDIO_HEADER,
                    packet_type: RTP_PAYLOAD_TYPE_AUDIO,
                    sequence_number: 3,
                    timestamp: 15,
                    ssrc: 0,
                },
                fourth_data,
            )
            .as_slice()
        ))
    );
    assert_eq!(
        payloader.poll_packet(),
        Ok(Some(
            construct_fec_packet(
                RtpAudioHeader {
                    header: RTP_AUDIO_HEADER,
                    packet_type: RTP_PAYLOAD_TYPE_AUDIO_FEC,
                    sequence_number: 4,
                    timestamp: 0,
                    ssrc: 0,
                },
                AudioFecHeader {
                    fec_shard_index: 0,
                    payload_type: RTP_PAYLOAD_TYPE_AUDIO,
                    base_sequence_number: 0,
                    base_timestamp: 0,
                    ssrc: 0
                },
                first_fec,
            )
            .as_slice()
        ))
    );
    assert_eq!(
        payloader.poll_packet(),
        Ok(Some(
            construct_fec_packet(
                RtpAudioHeader {
                    header: RTP_AUDIO_HEADER,
                    packet_type: RTP_PAYLOAD_TYPE_AUDIO_FEC,
                    sequence_number: 5,
                    timestamp: 0,
                    ssrc: 0,
                },
                AudioFecHeader {
                    fec_shard_index: 1,
                    payload_type: RTP_PAYLOAD_TYPE_AUDIO,
                    base_sequence_number: 0,
                    base_timestamp: 0,
                    ssrc: 0,
                },
                second_fec,
            )
            .as_slice()
        ))
    );
    assert_eq!(payloader.poll_packet(), Ok(None));
}

#[test]
pub fn test_audio_payloader_sunshine() {
    let frame1 = &SUNSHINE_PACKET1[RtpAudioHeader::SIZE..];
    let frame2 = &SUNSHINE_PACKET2[RtpAudioHeader::SIZE..];
    let frame3 = &SUNSHINE_PACKET3[RtpAudioHeader::SIZE..];
    let frame4 = &SUNSHINE_PACKET4[RtpAudioHeader::SIZE..];

    // TODO: frame len?
    let mut payloader = AudioPayloader::new(AudioPayloaderConfig {
        fec: true,
        frame_len: frame1.len(),
    });

    payloader.set_sequence_number(92);

    payloader.push_frame(460, frame1).unwrap();
    assert_eq!(payloader.poll_packet(), Ok(Some(SUNSHINE_PACKET1)));
    assert_eq!(payloader.poll_packet(), Ok(None));

    payloader.push_frame(465, frame2).unwrap();
    assert_eq!(payloader.poll_packet(), Ok(Some(SUNSHINE_PACKET2)));
    assert_eq!(payloader.poll_packet(), Ok(None));

    payloader.push_frame(470, frame3).unwrap();
    assert_eq!(payloader.poll_packet(), Ok(Some(SUNSHINE_PACKET3)));
    assert_eq!(payloader.poll_packet(), Ok(None));

    payloader.push_frame(475, frame4).unwrap();
    assert_eq!(payloader.poll_packet(), Ok(Some(SUNSHINE_PACKET4)));
    assert_eq!(payloader.poll_packet(), Ok(Some(SUNSHINE_PACKET5)));
    assert_eq!(payloader.poll_packet(), Ok(Some(SUNSHINE_PACKET6)));
    assert_eq!(payloader.poll_packet(), Ok(None));
}

#[test]
pub fn test_audio_depayloader_no_fec() {
    let mut depayloader = AudioDepayloader::new(AudioDepayloaderConfig { fec: false });

    let first_data = &[0, 1, 2, 3];
    let second_data = &[4, 5, 6, 7];
    let third_data = &[8, 9, 10, 11];
    let fourth_data = &[12, 13, 14, 15];

    assert_eq!(depayloader.poll_sample(), Ok(None));

    depayloader
        .handle_packet(&construct_data_packet(
            RtpAudioHeader {
                header: RTP_AUDIO_HEADER,
                packet_type: RTP_PAYLOAD_TYPE_AUDIO,
                sequence_number: 0,
                timestamp: 0,
                ssrc: 0,
            },
            first_data,
        ))
        .unwrap();
    assert_eq!(
        depayloader.poll_sample(),
        Ok(Some(AudioSample {
            timestamp: 0,
            buffer: first_data.to_vec()
        }))
    );
    assert_eq!(depayloader.poll_sample(), Ok(None));

    depayloader
        .handle_packet(&construct_data_packet(
            RtpAudioHeader {
                header: 128,
                packet_type: RTP_PAYLOAD_TYPE_AUDIO,
                sequence_number: 1,
                timestamp: 5,
                ssrc: 0,
            },
            second_data,
        ))
        .unwrap();
    assert_eq!(
        depayloader.poll_sample(),
        Ok(Some(AudioSample {
            timestamp: 5,
            buffer: second_data.to_vec(),
        }))
    );
    assert_eq!(depayloader.poll_sample(), Ok(None));

    depayloader
        .handle_packet(&construct_data_packet(
            RtpAudioHeader {
                header: 128,
                packet_type: RTP_PAYLOAD_TYPE_AUDIO,
                sequence_number: 2,
                timestamp: 10,
                ssrc: 0,
            },
            third_data,
        ))
        .unwrap();
    assert_eq!(
        depayloader.poll_sample(),
        Ok(Some(AudioSample {
            timestamp: 10,
            buffer: third_data.to_vec(),
        }))
    );
    assert_eq!(depayloader.poll_sample(), Ok(None));

    depayloader
        .handle_packet(&construct_data_packet(
            RtpAudioHeader {
                header: 128,
                packet_type: RTP_PAYLOAD_TYPE_AUDIO,
                sequence_number: 3,
                timestamp: 15,
                ssrc: 0,
            },
            fourth_data,
        ))
        .unwrap();
    assert_eq!(
        depayloader.poll_sample(),
        Ok(Some(AudioSample {
            timestamp: 15,
            buffer: fourth_data.to_vec(),
        }))
    );
    assert_eq!(depayloader.poll_sample(), Ok(None));
}

#[test]
fn test_audio_depayloader_no_fec_reorder() {
    let mut depayloader = AudioDepayloader::new(AudioDepayloaderConfig { fec: false });

    let first_data = &[0, 1, 2, 3];
    let second_data = &[4, 5, 6, 7];
    let third_data = &[8, 9, 10, 11];
    let fourth_data = &[12, 13, 14, 15];

    depayloader
        .handle_packet(&construct_data_packet(
            RtpAudioHeader {
                header: 128,
                packet_type: RTP_PAYLOAD_TYPE_AUDIO,
                sequence_number: 3,
                timestamp: 15,
                ssrc: 0,
            },
            fourth_data,
        ))
        .unwrap();
    assert_eq!(depayloader.poll_sample(), Ok(None));

    depayloader
        .handle_packet(&construct_data_packet(
            RtpAudioHeader {
                header: 128,
                packet_type: RTP_PAYLOAD_TYPE_AUDIO,
                sequence_number: 1,
                timestamp: 5,
                ssrc: 0,
            },
            second_data,
        ))
        .unwrap();
    assert_eq!(depayloader.poll_sample(), Ok(None));

    depayloader
        .handle_packet(&construct_data_packet(
            RtpAudioHeader {
                header: 128,
                packet_type: RTP_PAYLOAD_TYPE_AUDIO,
                sequence_number: 0,
                timestamp: 0,
                ssrc: 0,
            },
            first_data,
        ))
        .unwrap();
    assert_eq!(
        depayloader.poll_sample(),
        Ok(Some(AudioSample {
            timestamp: 0,
            buffer: first_data.to_vec(),
        }))
    );
    assert_eq!(
        depayloader.poll_sample(),
        Ok(Some(AudioSample {
            timestamp: 5,
            buffer: second_data.to_vec(),
        }))
    );
    assert_eq!(depayloader.poll_sample(), Ok(None));

    depayloader
        .handle_packet(&construct_data_packet(
            RtpAudioHeader {
                header: 128,
                packet_type: RTP_PAYLOAD_TYPE_AUDIO,
                sequence_number: 2,
                timestamp: 10,
                ssrc: 0,
            },
            third_data,
        ))
        .unwrap();
    assert_eq!(
        depayloader.poll_sample(),
        Ok(Some(AudioSample {
            timestamp: 10,
            buffer: third_data.to_vec(),
        }))
    );
    assert_eq!(
        depayloader.poll_sample(),
        Ok(Some(AudioSample {
            timestamp: 15,
            buffer: fourth_data.to_vec(),
        }))
    );
    assert_eq!(depayloader.poll_sample(), Ok(None));
}

#[test]
fn test_audio_depayloader_no_fec_packet_loss_no_recover() {
    let mut depayloader = AudioDepayloader::new(AudioDepayloaderConfig { fec: false });

    let first_data = &[0, 1, 2, 3];
    let second_data = &[4, 5, 6, 7];
    // third packet dropped
    let fourth_data = &[12, 13, 14, 15];
    let fifth_data = &[8, 9, 10, 11];

    depayloader
        .handle_packet(&construct_data_packet(
            RtpAudioHeader {
                header: 128,
                packet_type: RTP_PAYLOAD_TYPE_AUDIO,
                sequence_number: 3,
                timestamp: 15,
                ssrc: 0,
            },
            fourth_data,
        ))
        .unwrap();
    assert_eq!(depayloader.poll_sample(), Ok(None));

    depayloader
        .handle_packet(&construct_data_packet(
            RtpAudioHeader {
                header: 128,
                packet_type: RTP_PAYLOAD_TYPE_AUDIO,
                sequence_number: 1,
                timestamp: 5,
                ssrc: 0,
            },
            second_data,
        ))
        .unwrap();

    assert_eq!(depayloader.poll_sample(), Ok(None));

    depayloader
        .handle_packet(&construct_data_packet(
            RtpAudioHeader {
                header: 128,
                packet_type: RTP_PAYLOAD_TYPE_AUDIO,
                sequence_number: 0,
                timestamp: 0,
                ssrc: 0,
            },
            first_data,
        ))
        .unwrap();
    assert_eq!(
        depayloader.poll_sample(),
        Ok(Some(AudioSample {
            timestamp: 0,
            buffer: first_data.to_vec(),
        }))
    );
    assert_eq!(
        depayloader.poll_sample(),
        Ok(Some(AudioSample {
            timestamp: 5,
            buffer: second_data.to_vec(),
        }))
    );
    assert_eq!(depayloader.poll_sample(), Ok(None));

    depayloader.try_skip_samples().unwrap();
    assert_eq!(
        depayloader.poll_sample(),
        Ok(Some(AudioSample {
            timestamp: 15,
            buffer: fourth_data.to_vec(),
        }))
    );
    assert_eq!(depayloader.poll_sample(), Ok(None));

    // Receive another packet skipping packet 2
    depayloader
        .handle_packet(&construct_data_packet(
            RtpAudioHeader {
                header: 128,
                packet_type: RTP_PAYLOAD_TYPE_AUDIO,
                sequence_number: 4,
                timestamp: 20,
                ssrc: 0,
            },
            fifth_data,
        ))
        .unwrap();
    assert_eq!(
        depayloader.poll_sample(),
        Ok(Some(AudioSample {
            timestamp: 20,
            buffer: fifth_data.to_vec(),
        }))
    );
    assert_eq!(depayloader.poll_sample(), Ok(None));
}

#[test]
fn test_audio_depayloader() {
    let mut depayloader = AudioDepayloader::new(AudioDepayloaderConfig { fec: true });

    let first_data = &[0, 1, 2, 3];
    let second_data = &[4, 5, 6, 7];
    let third_data = &[8, 9, 10, 11];
    let fourth_data = &[12, 13, 14, 15];

    let fec_block1 = construct_fec_block(
        0,
        0,
        5,
        0,
        [first_data, second_data, third_data, fourth_data],
    );

    assert_eq!(depayloader.poll_sample(), Ok(None));

    depayloader.handle_packet(&fec_block1[0]).unwrap();
    assert_eq!(
        depayloader.poll_sample(),
        Ok(Some(AudioSample {
            timestamp: 0,
            buffer: first_data.to_vec(),
        }))
    );
    assert_eq!(depayloader.poll_sample(), Ok(None));

    depayloader.handle_packet(&fec_block1[1]).unwrap();
    assert_eq!(
        depayloader.poll_sample(),
        Ok(Some(AudioSample {
            timestamp: 5,
            buffer: second_data.to_vec(),
        }))
    );
    assert_eq!(depayloader.poll_sample(), Ok(None));

    depayloader.handle_packet(&fec_block1[2]).unwrap();
    assert_eq!(
        depayloader.poll_sample(),
        Ok(Some(AudioSample {
            timestamp: 10,
            buffer: third_data.to_vec(),
        }))
    );
    assert_eq!(depayloader.poll_sample(), Ok(None));

    depayloader.handle_packet(&fec_block1[3]).unwrap();
    assert_eq!(
        depayloader.poll_sample(),
        Ok(Some(AudioSample {
            timestamp: 15,
            buffer: fourth_data.to_vec(),
        }))
    );
    assert_eq!(depayloader.poll_sample(), Ok(None));

    depayloader.handle_packet(&fec_block1[4]).unwrap();
    assert_eq!(depayloader.poll_sample(), Ok(None));

    depayloader.handle_packet(&fec_block1[5]).unwrap();
    assert_eq!(depayloader.poll_sample(), Ok(None));

    let first_data = &[3, 2, 1, 0];
    let second_data = &[7, 6, 5, 4];
    let third_data = &[11, 10, 9, 8];
    let fourth_data = &[15, 14, 13, 12];

    let fec_block2 = construct_fec_block(
        4,
        20,
        5,
        0,
        [first_data, second_data, third_data, fourth_data],
    );

    depayloader.handle_packet(&fec_block2[0]).unwrap();
    assert_eq!(
        depayloader.poll_sample(),
        Ok(Some(AudioSample {
            timestamp: 20,
            buffer: first_data.to_vec(),
        }))
    );
    assert_eq!(depayloader.poll_sample(), Ok(None));

    depayloader.handle_packet(&fec_block2[1]).unwrap();
    assert_eq!(
        depayloader.poll_sample(),
        Ok(Some(AudioSample {
            timestamp: 25,
            buffer: second_data.to_vec(),
        }))
    );
    assert_eq!(depayloader.poll_sample(), Ok(None));

    depayloader.handle_packet(&fec_block2[2]).unwrap();
    assert_eq!(
        depayloader.poll_sample(),
        Ok(Some(AudioSample {
            timestamp: 30,
            buffer: third_data.to_vec(),
        }))
    );
    assert_eq!(depayloader.poll_sample(), Ok(None));

    depayloader.handle_packet(&fec_block2[3]).unwrap();
    assert_eq!(
        depayloader.poll_sample(),
        Ok(Some(AudioSample {
            timestamp: 35,
            buffer: fourth_data.to_vec(),
        }))
    );
    assert_eq!(depayloader.poll_sample(), Ok(None));

    depayloader.handle_packet(&fec_block2[4]).unwrap();
    depayloader.handle_packet(&fec_block2[5]).unwrap();

    assert_eq!(depayloader.poll_sample(), Ok(None));
}

#[test]
fn test_audio_depayloader_reorder() {
    let mut depayloader = AudioDepayloader::new(AudioDepayloaderConfig { fec: true });

    let first_data1 = &[0, 1, 2, 3];
    let second_data1 = &[4, 5, 6, 7];
    let third_data1 = &[8, 9, 10, 11];
    let fourth_data1 = &[12, 13, 14, 15];

    let fec_block1 = construct_fec_block(
        0,
        0,
        5,
        0,
        [first_data1, second_data1, third_data1, fourth_data1],
    );

    let first_data2 = &[3, 2, 1, 0];
    let second_data2 = &[7, 6, 5, 4];
    let third_data2 = &[11, 10, 9, 8];
    let fourth_data2 = &[15, 14, 13, 12];

    let fec_block2 = construct_fec_block(
        4,
        20,
        5,
        0,
        [first_data2, second_data2, third_data2, fourth_data2],
    );

    depayloader.handle_packet(&fec_block1[2]).unwrap();
    assert_eq!(depayloader.poll_sample(), Ok(None));

    depayloader.handle_packet(&fec_block1[0]).unwrap();
    assert_eq!(
        depayloader.poll_sample(),
        Ok(Some(AudioSample {
            timestamp: 0,
            buffer: first_data1.to_vec(),
        }))
    );
    assert_eq!(depayloader.poll_sample(), Ok(None));

    depayloader.handle_packet(&fec_block2[2]).unwrap();
    assert_eq!(depayloader.poll_sample(), Ok(None));

    depayloader.handle_packet(&fec_block1[1]).unwrap();
    assert_eq!(
        depayloader.poll_sample(),
        Ok(Some(AudioSample {
            timestamp: 5,
            buffer: second_data1.to_vec(),
        }))
    );
    assert_eq!(
        depayloader.poll_sample(),
        Ok(Some(AudioSample {
            timestamp: 10,
            buffer: third_data1.to_vec(),
        }))
    );
    assert_eq!(depayloader.poll_sample(), Ok(None));

    depayloader.handle_packet(&fec_block2[0]).unwrap();
    assert_eq!(depayloader.poll_sample(), Ok(None));

    depayloader.handle_packet(&fec_block1[3]).unwrap();
    assert_eq!(
        depayloader.poll_sample(),
        Ok(Some(AudioSample {
            timestamp: 15,
            buffer: fourth_data1.to_vec(),
        }))
    );
    assert_eq!(
        depayloader.poll_sample(),
        Ok(Some(AudioSample {
            timestamp: 20,
            buffer: first_data2.to_vec(),
        }))
    );
    assert_eq!(depayloader.poll_sample(), Ok(None));

    depayloader.handle_packet(&fec_block2[3]).unwrap();
    assert_eq!(depayloader.poll_sample(), Ok(None));

    depayloader.handle_packet(&fec_block2[1]).unwrap();
    assert_eq!(
        depayloader.poll_sample(),
        Ok(Some(AudioSample {
            timestamp: 25,
            buffer: second_data2.to_vec(),
        }))
    );
    assert_eq!(
        depayloader.poll_sample(),
        Ok(Some(AudioSample {
            timestamp: 30,
            buffer: third_data2.to_vec(),
        }))
    );
    assert_eq!(
        depayloader.poll_sample(),
        Ok(Some(AudioSample {
            timestamp: 35,
            buffer: fourth_data2.to_vec(),
        }))
    );
    assert_eq!(depayloader.poll_sample(), Ok(None));
}

#[test]
fn test_audio_depayloader_packet_loss_no_recover() {
    let mut depayloader = AudioDepayloader::new(AudioDepayloaderConfig { fec: true });

    let first_data = &[0, 1, 2, 3];
    let second_data = &[4, 5, 6, 7];
    let third_data = &[8, 9, 10, 11];
    let fourth_data = &[12, 13, 14, 15];

    let fec_block1 = construct_fec_block(
        0,
        0,
        5,
        0,
        [first_data, second_data, third_data, fourth_data],
    );

    depayloader.handle_packet(&fec_block1[1]).unwrap();
    assert_eq!(depayloader.poll_sample(), Ok(None));

    depayloader.handle_packet(&fec_block1[3]).unwrap();
    assert_eq!(depayloader.poll_sample(), Ok(None));

    depayloader.handle_packet(&fec_block1[5]).unwrap();
    assert_eq!(depayloader.poll_sample(), Ok(None));

    let fec_block2 = construct_fec_block(
        4,
        20,
        5,
        0,
        [first_data, second_data, third_data, fourth_data],
    );

    depayloader.handle_packet(&fec_block2[0]).unwrap();
    assert_eq!(depayloader.poll_sample(), Ok(None));

    depayloader.try_skip_samples().unwrap();
    assert_eq!(
        depayloader.poll_sample(),
        Ok(Some(AudioSample {
            timestamp: 5,
            buffer: second_data.to_vec(),
        }))
    );
    assert_eq!(depayloader.poll_sample(), Ok(None));

    depayloader.try_skip_samples().unwrap();
    assert_eq!(
        depayloader.poll_sample(),
        Ok(Some(AudioSample {
            timestamp: 15,
            buffer: fourth_data.to_vec(),
        }))
    );
    assert_eq!(
        depayloader.poll_sample(),
        Ok(Some(AudioSample {
            timestamp: 20,
            buffer: first_data.to_vec(),
        }))
    );
    assert_eq!(depayloader.poll_sample(), Ok(None));
}

#[test]
fn test_audio_depayloader_packet_loss_recover() {
    let mut depayloader = AudioDepayloader::new(AudioDepayloaderConfig { fec: true });

    let first_data = &[0, 1, 2, 3];
    let second_data = &[4, 5, 6, 7];
    let third_data = &[8, 9, 10, 11];
    let fourth_data = &[12, 13, 14, 15];

    let fec_block1 = construct_fec_block(
        0,
        0,
        5,
        0,
        [first_data, second_data, third_data, fourth_data],
    );

    depayloader.handle_packet(&fec_block1[1]).unwrap();
    assert_eq!(depayloader.poll_sample(), Ok(None));

    depayloader.handle_packet(&fec_block1[2]).unwrap();
    assert_eq!(depayloader.poll_sample(), Ok(None));

    depayloader.handle_packet(&fec_block1[3]).unwrap();
    assert_eq!(depayloader.poll_sample(), Ok(None));

    depayloader.handle_packet(&fec_block1[5]).unwrap();
    assert_eq!(
        depayloader.poll_sample(),
        Ok(Some(AudioSample {
            timestamp: 0,
            buffer: first_data.to_vec(),
        }))
    );
    assert_eq!(
        depayloader.poll_sample(),
        Ok(Some(AudioSample {
            timestamp: 5,
            buffer: second_data.to_vec(),
        }))
    );
    assert_eq!(
        depayloader.poll_sample(),
        Ok(Some(AudioSample {
            timestamp: 10,
            buffer: third_data.to_vec(),
        }))
    );
    assert_eq!(
        depayloader.poll_sample(),
        Ok(Some(AudioSample {
            timestamp: 15,
            buffer: fourth_data.to_vec(),
        }))
    );
    assert_eq!(depayloader.poll_sample(), Ok(None));
}

#[test]
fn test_audio_depayloader_packet_loss_recover_use_polled_packet() {
    let mut depayloader = AudioDepayloader::new(AudioDepayloaderConfig { fec: true });

    let first_data = &[0, 1, 2, 3];
    let second_data = &[4, 5, 6, 7];
    let third_data = &[8, 9, 10, 11];
    let fourth_data = &[12, 13, 14, 15];

    let fec_block1 = construct_fec_block(
        0,
        0,
        5,
        0,
        [first_data, second_data, third_data, fourth_data],
    );

    depayloader.handle_packet(&fec_block1[0]).unwrap();
    assert_eq!(
        depayloader.poll_sample(),
        Ok(Some(AudioSample {
            timestamp: 0,
            buffer: first_data.to_vec(),
        }))
    );
    assert_eq!(depayloader.poll_sample(), Ok(None));

    depayloader.handle_packet(&fec_block1[2]).unwrap();
    assert_eq!(depayloader.poll_sample(), Ok(None));

    depayloader.handle_packet(&fec_block1[3]).unwrap();
    assert_eq!(depayloader.poll_sample(), Ok(None));

    println!("depayloader before last packet: {depayloader:#?}");
    depayloader.handle_packet(&fec_block1[5]).unwrap();
    println!("depayloader after last packet: {depayloader:#?}");
    assert_eq!(
        depayloader.poll_sample(),
        Ok(Some(AudioSample {
            timestamp: 5,
            buffer: second_data.to_vec(),
        }))
    );
    assert_eq!(
        depayloader.poll_sample(),
        Ok(Some(AudioSample {
            timestamp: 10,
            buffer: third_data.to_vec(),
        }))
    );
    assert_eq!(
        depayloader.poll_sample(),
        Ok(Some(AudioSample {
            timestamp: 15,
            buffer: fourth_data.to_vec(),
        }))
    );
    assert_eq!(depayloader.poll_sample(), Ok(None));
}

#[test]
fn test_audio_depayloader_late_data_packet_fec_recovery() {
    let mut depayloader = AudioDepayloader::new(AudioDepayloaderConfig { fec: true });

    let first_data = &[0, 1, 2, 3];
    let second_data = &[4, 5, 6, 7];
    let third_data = &[8, 9, 10, 11];
    let fourth_data = &[12, 13, 14, 15];

    let fec_block = construct_fec_block(
        0,
        0,
        5,
        0,
        [first_data, second_data, third_data, fourth_data],
    );

    depayloader.handle_packet(&fec_block[0]).unwrap();
    assert_eq!(
        depayloader.poll_sample(),
        Ok(Some(AudioSample {
            timestamp: 0,
            buffer: first_data.to_vec(),
        }))
    );
    assert_eq!(depayloader.poll_sample(), Ok(None));

    depayloader.handle_packet(&fec_block[4]).unwrap();
    assert_eq!(depayloader.poll_sample(), Ok(None));

    depayloader.handle_packet(&fec_block[5]).unwrap();
    assert_eq!(depayloader.poll_sample(), Ok(None));

    depayloader.handle_packet(&fec_block[1]).unwrap();
    assert_eq!(
        depayloader.poll_sample(),
        Ok(Some(AudioSample {
            timestamp: 5,
            buffer: second_data.to_vec(),
        }))
    );
    assert_eq!(
        depayloader.poll_sample(),
        Ok(Some(AudioSample {
            timestamp: 10,
            buffer: third_data.to_vec(),
        }))
    );
    assert_eq!(
        depayloader.poll_sample(),
        Ok(Some(AudioSample {
            timestamp: 15,
            buffer: fourth_data.to_vec(),
        }))
    );
    assert_eq!(depayloader.poll_sample(), Ok(None));
}

#[test]
fn test_audio_depayloader_big_packet_loss() {
    // Testing with more than 100 packets lost

    let mut depayloader = AudioDepayloader::new(AudioDepayloaderConfig { fec: true });

    let first_data = &[0, 1, 2, 3];
    let second_data = &[4, 5, 6, 7];
    let third_data = &[8, 9, 10, 11];
    let fourth_data = &[12, 13, 14, 15];

    let fec_block1 = construct_fec_block(
        0,
        0,
        5,
        0,
        [first_data, second_data, third_data, fourth_data],
    );

    depayloader.handle_packet(&fec_block1[0]).unwrap();
    assert_eq!(
        depayloader.poll_sample(),
        Ok(Some(AudioSample {
            timestamp: 0,
            buffer: first_data.to_vec(),
        }))
    );
    assert_eq!(depayloader.poll_sample(), Ok(None));

    let fec_block2 = construct_fec_block(
        100,
        20,
        5,
        0,
        [first_data, second_data, third_data, fourth_data],
    );

    depayloader.handle_packet(&fec_block2[0]).unwrap();
    assert_eq!(depayloader.poll_sample(), Ok(None));

    depayloader.try_skip_samples().unwrap();
    assert_eq!(
        depayloader.poll_sample(),
        Ok(Some(AudioSample {
            timestamp: 20,
            buffer: first_data.to_vec(),
        }))
    );
    assert_eq!(depayloader.poll_sample(), Ok(None));

    depayloader.handle_packet(&fec_block2[1]).unwrap();
    assert_eq!(
        depayloader.poll_sample(),
        Ok(Some(AudioSample {
            timestamp: 25,
            buffer: second_data.to_vec(),
        }))
    );
    assert_eq!(depayloader.poll_sample(), Ok(None));
}

#[test]
fn test_audio_depayloader_sunshine() {
    // This tests using real audio data received from sunshine

    let mut depayloader = AudioDepayloader::new(AudioDepayloaderConfig { fec: true });

    let expected1 = &SUNSHINE_PACKET1[RtpAudioHeader::SIZE..];
    let expected2 = &SUNSHINE_PACKET2[RtpAudioHeader::SIZE..];
    let expected3 = &SUNSHINE_PACKET3[RtpAudioHeader::SIZE..];
    let expected4 = &SUNSHINE_PACKET4[RtpAudioHeader::SIZE..];

    depayloader.handle_packet(SUNSHINE_PACKET1).unwrap();
    depayloader.try_skip_samples().unwrap();
    assert_eq!(
        depayloader.poll_sample(),
        Ok(Some(AudioSample {
            timestamp: 460,
            buffer: expected1.to_vec(),
        }))
    );
    assert_eq!(depayloader.poll_sample(), Ok(None));

    depayloader.handle_packet(SUNSHINE_PACKET2).unwrap();
    assert_eq!(
        depayloader.poll_sample(),
        Ok(Some(AudioSample {
            timestamp: 465,
            buffer: expected2.to_vec(),
        }))
    );
    assert_eq!(depayloader.poll_sample(), Ok(None));

    depayloader.handle_packet(SUNSHINE_PACKET3).unwrap();
    assert_eq!(
        depayloader.poll_sample(),
        Ok(Some(AudioSample {
            timestamp: 470,
            buffer: expected3.to_vec(),
        }))
    );
    assert_eq!(depayloader.poll_sample(), Ok(None));

    depayloader.handle_packet(SUNSHINE_PACKET4).unwrap();
    assert_eq!(
        depayloader.poll_sample(),
        Ok(Some(AudioSample {
            timestamp: 475,
            buffer: expected4.to_vec(),
        }))
    );
    assert_eq!(depayloader.poll_sample(), Ok(None));

    // Make sure that fec is also working because of the reed solomon parity matrix
    let mut depayloader = AudioDepayloader::new(AudioDepayloaderConfig { fec: true });

    depayloader.handle_packet(SUNSHINE_PACKET1).unwrap();
    depayloader.try_skip_samples().unwrap();
    assert_eq!(
        depayloader.poll_sample(),
        Ok(Some(AudioSample {
            timestamp: 460,
            buffer: expected1.to_vec(),
        }))
    );
    assert_eq!(depayloader.poll_sample(), Ok(None));

    depayloader.handle_packet(SUNSHINE_PACKET3).unwrap();
    assert_eq!(depayloader.poll_sample(), Ok(None));

    depayloader.handle_packet(SUNSHINE_PACKET6).unwrap();
    assert_eq!(depayloader.poll_sample(), Ok(None));

    depayloader.handle_packet(SUNSHINE_PACKET5).unwrap();
    assert_eq!(
        depayloader.poll_sample(),
        Ok(Some(AudioSample {
            timestamp: 465,
            buffer: expected2.to_vec(),
        }))
    );
    assert_eq!(
        depayloader.poll_sample(),
        Ok(Some(AudioSample {
            timestamp: 470,
            buffer: expected3.to_vec(),
        }))
    );
    assert_eq!(
        depayloader.poll_sample(),
        Ok(Some(AudioSample {
            timestamp: 475,
            buffer: expected4.to_vec(),
        }))
    );
    assert_eq!(depayloader.poll_sample(), Ok(None));
}

const SUNSHINE_PACKET1: &[u8] = &[
    128, 97, 0, 92, 0, 0, 1, 204, 0, 0, 0, 0, 236, 192, 188, 221, 96, 231, 110, 50, 143, 255, 234,
    169, 224, 205, 95, 251, 95, 213, 147, 193, 40, 191, 31, 132, 172, 94, 230, 187, 98, 188, 91,
    111, 194, 63, 43, 185, 3, 36, 134, 54, 0, 217, 164, 189, 84, 182, 182, 150, 225, 28, 112, 184,
    60, 178, 104, 207, 60, 25, 171, 251, 39, 225, 4, 29, 243, 17, 77, 25, 70, 218, 35, 166, 255,
    96, 225, 152, 172, 62, 129, 14, 208, 28, 121, 178, 154, 161, 156, 37, 233, 232, 112, 21, 114,
    201, 191, 216, 254, 101, 70, 82, 6, 11, 203, 111, 38, 78, 13, 9, 139, 80, 247, 215, 26, 50, 40,
    219, 24, 34, 14, 190, 198, 151, 189, 253, 89, 131, 10, 169, 44, 246, 183, 154, 33, 118, 9, 22,
    2, 152, 125, 72, 243, 82, 97, 220, 13, 199, 36, 7, 145, 160, 165, 201, 235, 132, 195, 240, 240,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 13, 155, 32, 180,
    34, 133, 71, 92, 26, 192, 206, 77, 241, 224, 0, 3, 118, 48, 93, 191, 56, 23, 181, 212, 236,
    241, 34, 126, 154, 160, 107, 178, 175, 225, 171, 53, 158, 55, 149, 82, 210, 193, 55, 253, 45,
    139, 232, 25, 72, 70, 162, 245, 38, 58, 226, 48, 80, 156, 105, 210, 237, 90, 248, 114, 222, 16,
    243, 135, 26, 14, 3, 92, 105, 154, 191, 34, 22, 202, 21, 68, 205, 76, 49, 234, 50, 85, 26, 229,
    244, 100, 24, 225, 95, 110, 35, 139, 112, 203, 121, 64, 68, 182, 128, 11, 64, 7, 23, 151, 184,
    218, 230, 133, 89, 0, 1, 79, 30, 9, 144, 212, 217, 196, 131, 166, 15, 38, 176, 151, 138, 140,
    209, 82, 162, 214, 136, 248, 215, 27,
];
const SUNSHINE_PACKET2: &[u8] = &[
    128, 97, 0, 93, 0, 0, 1, 209, 0, 0, 0, 0, 236, 192, 100, 18, 9, 58, 166, 5, 175, 250, 209, 230,
    234, 215, 62, 41, 15, 142, 58, 119, 77, 240, 65, 137, 66, 40, 222, 213, 128, 217, 104, 161,
    141, 53, 83, 188, 142, 112, 99, 169, 165, 220, 20, 200, 60, 241, 58, 79, 2, 115, 245, 8, 19,
    172, 168, 151, 85, 223, 35, 129, 181, 145, 172, 252, 106, 77, 12, 250, 163, 40, 132, 112, 124,
    236, 238, 0, 85, 182, 154, 255, 227, 16, 42, 220, 241, 86, 41, 144, 216, 172, 253, 24, 168, 31,
    175, 32, 119, 50, 42, 230, 249, 220, 174, 89, 10, 35, 22, 55, 80, 92, 155, 122, 245, 26, 183,
    236, 192, 155, 61, 162, 179, 136, 210, 242, 146, 44, 131, 194, 101, 48, 221, 228, 33, 149, 127,
    218, 198, 8, 230, 117, 158, 136, 200, 57, 101, 186, 231, 152, 253, 199, 213, 59, 165, 5, 236,
    236, 99, 35, 169, 55, 98, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 126, 163, 202, 152, 193, 43, 73, 88, 94, 16, 228, 54, 104, 102, 239, 32, 119,
    177, 206, 252, 213, 217, 214, 254, 88, 156, 203, 65, 193, 54, 205, 186, 111, 156, 243, 121, 97,
    199, 71, 123, 158, 94, 126, 248, 62, 100, 165, 52, 139, 167, 103, 185, 64, 140, 124, 71, 14,
    103, 130, 230, 6, 56, 25, 71, 200, 152, 235, 209, 233, 75, 114, 37, 54, 223, 248, 101, 193,
    145, 65, 164, 216, 37, 67, 222, 114, 246, 150, 16, 49, 94, 23, 183, 77, 111, 75, 65, 116, 146,
    217, 145, 5, 157, 32, 153, 209, 240, 183, 118, 68, 244, 63, 238, 60, 163, 11, 25, 205, 180, 42,
    195, 130, 37, 39, 153, 209, 237, 0, 41, 99, 219, 27,
];
const SUNSHINE_PACKET3: &[u8] = &[
    128, 97, 0, 94, 0, 0, 1, 214, 0, 0, 0, 0, 236, 194, 4, 89, 184, 23, 197, 69, 212, 86, 139, 238,
    76, 36, 71, 82, 196, 127, 148, 56, 157, 38, 50, 120, 27, 88, 48, 147, 159, 113, 72, 197, 12,
    45, 89, 141, 110, 163, 130, 176, 48, 50, 224, 243, 179, 150, 147, 159, 237, 199, 227, 127, 47,
    61, 247, 66, 35, 26, 19, 13, 251, 101, 180, 51, 83, 176, 63, 251, 200, 56, 157, 12, 154, 186,
    211, 243, 17, 46, 65, 32, 80, 73, 153, 159, 175, 76, 184, 219, 100, 255, 123, 238, 156, 124,
    67, 12, 217, 226, 227, 92, 171, 218, 98, 14, 41, 226, 79, 110, 88, 185, 80, 101, 108, 199, 255,
    236, 140, 116, 254, 213, 250, 166, 78, 102, 160, 39, 142, 224, 112, 179, 246, 74, 40, 32, 110,
    213, 62, 201, 233, 27, 224, 62, 233, 61, 224, 60, 149, 187, 132, 54, 108, 160, 206, 143, 168,
    98, 177, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2,
    216, 113, 87, 60, 8, 223, 38, 133, 68, 113, 109, 108, 190, 246, 241, 160, 153, 148, 53, 94,
    140, 254, 188, 4, 234, 209, 165, 103, 157, 39, 226, 76, 94, 200, 189, 139, 75, 158, 200, 107,
    117, 33, 73, 117, 22, 205, 43, 218, 173, 36, 251, 115, 185, 123, 69, 23, 130, 42, 243, 26, 98,
    172, 148, 57, 121, 37, 158, 225, 17, 101, 156, 238, 209, 92, 202, 146, 233, 204, 205, 4, 141,
    221, 244, 198, 67, 229, 37, 197, 190, 105, 8, 214, 12, 232, 191, 24, 33, 209, 18, 144, 81, 152,
    85, 1, 160, 251, 247, 252, 63, 195, 247, 171, 225, 133, 25, 164, 178, 239, 190, 136, 63, 70,
    109, 234, 172, 133, 190, 121, 62, 147, 23, 93, 235, 27,
];
const SUNSHINE_PACKET4: &[u8] = &[
    128, 97, 0, 95, 0, 0, 1, 219, 0, 0, 0, 0, 236, 197, 20, 180, 209, 114, 64, 90, 69, 154, 221,
    152, 41, 195, 182, 205, 93, 97, 113, 163, 229, 134, 89, 99, 204, 182, 175, 86, 119, 226, 146,
    184, 36, 105, 152, 50, 235, 70, 121, 7, 229, 211, 86, 249, 67, 112, 34, 253, 142, 205, 158, 69,
    197, 95, 235, 2, 146, 42, 195, 236, 35, 204, 239, 199, 143, 10, 20, 157, 211, 13, 51, 202, 194,
    187, 207, 222, 180, 111, 175, 179, 173, 47, 185, 233, 187, 248, 39, 228, 140, 81, 25, 14, 36,
    247, 67, 243, 116, 70, 12, 11, 203, 179, 166, 25, 182, 92, 197, 212, 241, 29, 173, 177, 181,
    89, 68, 24, 29, 207, 134, 7, 245, 13, 219, 65, 226, 52, 103, 84, 46, 2, 66, 153, 114, 178, 169,
    123, 224, 103, 143, 83, 212, 255, 235, 1, 51, 148, 246, 85, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 2, 210, 124, 221, 57, 98, 69, 216, 222, 150, 44, 111, 216, 251, 17, 77, 245, 6, 60, 47, 110,
    12, 183, 230, 135, 189, 221, 149, 72, 111, 46, 224, 124, 184, 206, 26, 45, 192, 234, 110, 217,
    230, 173, 124, 81, 100, 92, 185, 174, 201, 25, 162, 92, 97, 8, 219, 221, 57, 219, 11, 124, 255,
    65, 245, 153, 31, 135, 230, 75, 78, 52, 183, 5, 4, 176, 43, 16, 199, 220, 189, 164, 9, 152, 92,
    19, 43, 183, 20, 129, 56, 50, 209, 6, 234, 128, 179, 19, 14, 46, 93, 17, 57, 7, 9, 2, 126, 145,
    205, 27, 210, 231, 57, 41, 99, 180, 40, 249, 236, 212, 214, 91, 197, 94, 110, 196, 111, 27,
];
const SUNSHINE_PACKET5: &[u8] = &[
    128, 127, 0, 96, 0, 0, 0, 0, 0, 0, 0, 0, 0, 97, 0, 92, 0, 0, 1, 204, 0, 0, 0, 0, 236, 134, 62,
    197, 210, 56, 0, 131, 107, 9, 86, 190, 239, 22, 23, 187, 153, 111, 27, 199, 159, 197, 212, 222,
    222, 91, 89, 216, 144, 37, 98, 19, 95, 49, 30, 193, 74, 31, 10, 208, 217, 217, 44, 197, 141,
    37, 22, 47, 158, 1, 105, 115, 107, 42, 224, 170, 107, 169, 100, 73, 1, 154, 191, 153, 170, 55,
    219, 196, 32, 102, 153, 152, 238, 75, 11, 178, 48, 109, 113, 174, 1, 205, 80, 67, 186, 50, 101,
    215, 45, 39, 1, 102, 124, 126, 116, 59, 199, 55, 85, 32, 130, 158, 104, 29, 47, 207, 209, 61,
    161, 215, 116, 84, 73, 35, 246, 32, 60, 189, 114, 157, 44, 23, 106, 95, 3, 173, 122, 186, 187,
    174, 73, 46, 145, 73, 22, 248, 169, 193, 183, 120, 169, 214, 199, 237, 218, 50, 59, 208, 81,
    176, 94, 5, 65, 189, 74, 236, 148, 40, 21, 65, 165, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 41, 32, 70, 209, 37, 217, 125, 206, 32, 92, 51, 58, 199, 192, 33, 12, 60,
    216, 216, 250, 198, 189, 50, 207, 13, 214, 32, 37, 62, 253, 224, 126, 235, 183, 17, 59, 243,
    104, 20, 144, 131, 218, 21, 18, 108, 216, 52, 142, 206, 226, 169, 246, 91, 93, 228, 75, 31, 72,
    45, 92, 237, 200, 186, 165, 19, 107, 42, 131, 0, 67, 80, 207, 231, 139, 237, 84, 233, 108, 119,
    15, 203, 241, 220, 98, 84, 175, 124, 250, 168, 38, 134, 240, 181, 199, 121, 237, 117, 27, 207,
    1, 169, 49, 69, 152, 131, 100, 109, 233, 61, 44, 195, 29, 83, 83, 110, 16, 10, 167, 211, 176,
    48, 96, 57, 110, 40, 91, 183, 199, 159, 203, 140, 173, 19, 40, 41, 94, 39, 27,
];
const SUNSHINE_PACKET6: &[u8] = &[
    128, 127, 0, 97, 0, 0, 0, 0, 0, 0, 0, 0, 1, 97, 0, 92, 0, 0, 1, 204, 0, 0, 0, 0, 236, 27, 13,
    199, 139, 81, 203, 246, 135, 42, 245, 191, 224, 46, 135, 45, 246, 71, 185, 204, 216, 209, 48,
    82, 25, 140, 50, 160, 121, 16, 190, 144, 238, 14, 28, 22, 225, 164, 246, 237, 138, 44, 36, 123,
    78, 152, 155, 156, 134, 22, 52, 191, 3, 176, 70, 145, 141, 11, 12, 205, 39, 213, 34, 196, 202,
    245, 239, 216, 22, 38, 58, 210, 152, 158, 166, 192, 81, 38, 28, 111, 26, 94, 143, 110, 237, 33,
    238, 78, 232, 175, 14, 131, 178, 114, 230, 6, 116, 127, 141, 151, 151, 136, 178, 78, 73, 140,
    54, 161, 59, 82, 168, 104, 244, 55, 67, 142, 203, 78, 134, 238, 132, 67, 240, 254, 238, 117,
    250, 234, 91, 178, 124, 186, 124, 34, 73, 17, 129, 46, 120, 62, 151, 60, 138, 88, 79, 93, 229,
    116, 130, 44, 228, 56, 141, 140, 142, 40, 60, 173, 214, 169, 184, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 138, 253, 197, 59, 71, 200, 108, 234, 181, 95, 99, 94, 181,
    69, 196, 194, 138, 52, 249, 112, 197, 211, 182, 86, 225, 69, 234, 116, 182, 123, 241, 0, 102,
    196, 189, 159, 243, 18, 26, 244, 178, 204, 63, 137, 41, 117, 232, 150, 173, 178, 247, 134, 112,
    231, 115, 8, 97, 56, 105, 173, 156, 74, 21, 77, 128, 174, 239, 175, 104, 117, 31, 210, 99, 128,
    199, 193, 247, 173, 225, 87, 5, 108, 110, 29, 244, 49, 141, 172, 129, 96, 222, 132, 105, 208,
    61, 32, 164, 152, 20, 165, 60, 134, 8, 166, 61, 130, 78, 134, 166, 124, 9, 123, 120, 119, 183,
    78, 2, 88, 220, 69, 196, 123, 114, 73, 172, 89, 232, 94, 76, 167, 186, 54, 75, 87, 40, 169,
    221, 27,
];
