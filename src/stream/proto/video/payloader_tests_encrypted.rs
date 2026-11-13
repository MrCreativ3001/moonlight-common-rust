use std::{sync::Arc, time::Duration};

use crate::{
    ServerVersion,
    crypto::rustcrypto::RustCryptoBackend,
    stream::{
        AesIv, AesKey,
        proto::video::{
            packet::{RtpVideoHeader, VideoFrameHeader, VideoHeader, video_header_size},
            payloader::{VideoPayloader, VideoPayloaderConfig, VideoPayloaderFecConfig},
        },
        video::{FrameType, ServerFrameType},
    },
};

fn sunshine_gen_7_431() -> ServerVersion {
    ServerVersion::new(7, 1, 431, -1)
}

#[test]
fn nofec() {
    let aes_key = AesKey([0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15]);
    let payload_size = 64;
    let frame: Vec<u8> = (0..(payload_size as u8 * 2 - VideoFrameHeader::SIZE as u8))
        .into_iter()
        .collect::<Vec<_>>();

    let mut payloader = VideoPayloader::new(
        VideoPayloaderConfig {
            server_version: sunshine_gen_7_431(),
            packet_size: payload_size + video_header_size(true),
            max_data_shards_per_block: 100,
            fec: None,
            encryption: Some((aes_key, AesIv(0))),
        },
        Arc::new(RustCryptoBackend),
    );

    payloader
        .push_frame(0, None, ServerFrameType::Idr, &frame)
        .unwrap();

    assert_eq!(
        payloader.poll_packet().as_deref(),
        Some(
            [
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 86, 1, 0, 0, 0, 17, 255, 67, 34, 47, 206, 121,
                232, 89, 129, 59, 38, 219, 83, 241, 143, 79, 51, 119, 45, 214, 139, 54, 187, 252,
                121, 41, 134, 86, 37, 236, 88, 25, 156, 185, 250, 162, 235, 67, 230, 99, 220, 200,
                15, 5, 177, 237, 93, 255, 62, 28, 129, 139, 15, 209, 112, 175, 177, 166, 217, 133,
                24, 175, 196, 236, 243, 112, 76, 4, 202, 184, 23, 37, 89, 56, 111, 64, 26, 235, 95,
                48, 161, 187, 194, 239, 154, 182, 182, 75, 39, 24, 32, 108, 47, 214, 227, 170, 108,
                46, 246, 241, 214, 146, 117, 170, 78, 208, 84, 142, 24, 186, 20
            ]
            .as_slice()
        )
    );
    assert_eq!(
        payloader.poll_packet().as_deref(),
        Some(
            [
                1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 86, 1, 0, 0, 0, 118, 128, 182, 249, 237, 80, 140,
                110, 86, 215, 154, 171, 26, 247, 27, 180, 95, 226, 170, 198, 163, 161, 92, 24, 9,
                245, 111, 182, 105, 155, 160, 250, 151, 162, 48, 120, 31, 91, 218, 100, 12, 184,
                78, 51, 49, 248, 181, 119, 75, 224, 139, 98, 86, 82, 121, 229, 8, 246, 104, 233,
                58, 140, 205, 111, 28, 165, 16, 45, 86, 251, 233, 131, 207, 108, 145, 168, 48, 114,
                216, 187, 163, 105, 81, 30, 248, 21, 110, 141, 13, 214, 232, 25, 1, 5, 119, 213,
                211, 219, 44, 83, 192, 32, 24, 217, 192, 40, 214, 80, 55, 17, 88, 107
            ]
            .as_slice()
        )
    );
    assert_eq!(payloader.poll_packet().as_deref(), None);
}

#[test]
fn fec() {
    let aes_key = AesKey([0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15]);
    let payload_size = 64;
    let frame: Vec<u8> = (0..(payload_size as u8 * 2 - VideoFrameHeader::SIZE as u8))
        .into_iter()
        .collect::<Vec<_>>();

    let mut payloader = VideoPayloader::new(
        VideoPayloaderConfig {
            server_version: sunshine_gen_7_431(),
            packet_size: payload_size + video_header_size(true),
            max_data_shards_per_block: 100,
            fec: Some(VideoPayloaderFecConfig {
                fec_percentage: 0,
                min_required_fec_packets: 1,
            }),
            encryption: Some((aes_key, AesIv(0))),
        },
        Arc::new(RustCryptoBackend),
    );

    payloader
        .push_frame(0, None, ServerFrameType::Idr, &frame)
        .unwrap();

    assert_eq!(
        payloader.poll_packet().as_deref(),
        Some(
            [
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 86, 1, 0, 0, 0, 226, 121, 140, 45, 18, 213, 49,
                120, 18, 153, 220, 170, 160, 72, 108, 20, 79, 51, 119, 45, 214, 139, 54, 187, 252,
                121, 41, 134, 86, 37, 236, 88, 25, 156, 185, 250, 162, 235, 67, 230, 99, 220, 200,
                15, 37, 178, 237, 93, 255, 62, 28, 129, 139, 15, 209, 112, 175, 177, 166, 217, 133,
                24, 175, 196, 236, 243, 112, 76, 4, 202, 184, 23, 37, 89, 56, 111, 64, 26, 235, 95,
                48, 161, 187, 194, 239, 154, 182, 182, 75, 39, 24, 32, 108, 47, 214, 227, 170, 108,
                46, 246, 241, 214, 146, 117, 170, 78, 208, 84, 142, 24, 186, 20
            ]
            .as_slice()
        )
    );
    assert_eq!(
        payloader.poll_packet().as_deref(),
        Some(
            [
                1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 86, 1, 0, 0, 0, 133, 6, 121, 246, 208, 75, 196,
                254, 29, 207, 125, 39, 97, 236, 134, 47, 95, 226, 170, 198, 163, 161, 92, 24, 9,
                245, 111, 182, 105, 155, 160, 250, 151, 162, 48, 120, 31, 91, 218, 100, 12, 184,
                78, 51, 17, 251, 181, 119, 75, 224, 139, 98, 86, 82, 121, 229, 8, 246, 104, 233,
                58, 140, 205, 111, 28, 165, 16, 45, 86, 251, 233, 131, 207, 108, 145, 168, 48, 114,
                216, 187, 163, 105, 81, 30, 248, 21, 110, 141, 13, 214, 232, 25, 1, 5, 119, 213,
                211, 219, 44, 83, 192, 32, 24, 217, 192, 40, 214, 80, 55, 17, 88, 107
            ]
            .as_slice()
        )
    );
    assert_eq!(
        payloader.poll_packet().as_deref(),
        Some(
            [
                2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 86, 1, 0, 0, 0, 94, 156, 44, 9, 217, 161, 94, 98,
                51, 107, 164, 175, 123, 92, 33, 255, 120, 245, 200, 130, 77, 4, 214, 207, 5, 4,
                123, 4, 239, 109, 192, 176, 16, 18, 50, 149, 122, 241, 115, 103, 82, 17, 224, 119,
                32, 201, 84, 98, 75, 167, 119, 145, 153, 2, 82, 137, 253, 9, 235, 66, 166, 195,
                145, 70, 108, 52, 110, 111, 153, 109, 160, 247, 242, 32, 217, 175, 4, 202, 29, 147,
                208, 26, 238, 93, 129, 246, 107, 254, 108, 241, 112, 101, 190, 69, 86, 34, 230, 18,
                123, 212, 37, 30, 102, 60, 168, 191, 148, 59, 178, 131, 109, 181
            ]
            .as_slice()
        )
    );
    assert_eq!(payloader.poll_packet().as_deref(), None);
}
