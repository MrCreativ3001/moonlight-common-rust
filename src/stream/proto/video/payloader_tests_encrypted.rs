use std::{sync::Arc, time::Duration};

use crate::{
    ServerVersion,
    crypto::rustcrypto::RustCryptoBackend,
    stream::{
        AesIv, AesKey,
        proto::video::{
            packet::{RtpVideoHeader, VideoFrameHeader, VideoHeader},
            payloader::{VideoPayloader, VideoPayloaderConfig},
        },
        video::{FrameType, ServerFrameType},
    },
};

fn sunshine_gen_7_431() -> ServerVersion {
    ServerVersion::new(7, 1, 431, -1)
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
fn nofec() {
    let aes_key = AesKey([0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15]);
    let packet_size = 64;
    let frame: Vec<u8> = (0..(packet_size as u8 * 2 - VideoFrameHeader::SIZE as u8))
        .into_iter()
        .collect::<Vec<_>>();

    let mut payloader = VideoPayloader::new(
        VideoPayloaderConfig {
            server_version: sunshine_gen_7_431(),
            packet_size,
            max_data_shards_per_block: 100,
            fec: None,
            encryption: Some((aes_key, AesIv(0))),
        },
        Arc::new(RustCryptoBackend),
    );

    payloader
        .push_frame(0, None, ServerFrameType::Idr, &frame)
        .unwrap();

    assert_eq!(payloader.poll_packet().as_deref(), Some([0, 0].as_slice()));
    assert_eq!(payloader.poll_packet().as_deref(), Some([0, 0].as_slice()));
    assert_eq!(payloader.poll_packet().as_deref(), None);
}
