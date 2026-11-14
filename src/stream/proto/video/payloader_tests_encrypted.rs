use std::sync::Arc;

use crate::{
    ServerVersion,
    crypto::rustcrypto::RustCryptoBackend,
    stream::{
        AesIv, AesKey,
        proto::video::{
            packet::{VideoFrameHeader, video_header_size},
            payloader::{VideoPayloader, VideoPayloaderConfig, VideoPayloaderFecConfig},
        },
        video::ServerFrameType,
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
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 86, 1, 0, 0, 0, 148, 79, 111, 40, 60, 193, 244,
                135, 60, 74, 56, 34, 152, 245, 148, 179, 79, 51, 119, 45, 214, 139, 54, 187, 252,
                121, 41, 134, 86, 37, 236, 88, 25, 156, 185, 250, 162, 235, 67, 230, 99, 220, 200,
                15, 5, 177, 237, 93, 254, 62, 28, 131, 203, 15, 209, 112, 175, 176, 164, 218, 129,
                29, 169, 195, 228, 250, 122, 71, 8, 199, 182, 24, 53, 72, 42, 124, 84, 15, 253, 72,
                41, 184, 161, 219, 179, 135, 168, 169, 107, 7, 56, 0, 76, 15, 246, 195, 138, 76,
                14, 214, 209, 246, 178, 85, 138, 110, 240, 116, 174, 56, 154, 52
            ]
            .as_slice()
        )
    );
    assert_eq!(
        payloader.poll_packet().as_deref(),
        Some(
            [
                1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 86, 1, 0, 0, 0, 26, 245, 213, 82, 136, 175, 109,
                94, 100, 153, 90, 250, 120, 192, 247, 72, 95, 226, 170, 198, 163, 161, 92, 24, 9,
                245, 111, 182, 105, 155, 160, 250, 151, 162, 48, 120, 31, 91, 218, 100, 12, 184,
                78, 51, 49, 248, 181, 119, 115, 217, 177, 89, 106, 111, 71, 218, 72, 183, 42, 170,
                126, 201, 139, 40, 84, 236, 90, 102, 26, 182, 167, 204, 159, 61, 195, 251, 100, 39,
                142, 236, 195, 9, 49, 126, 152, 117, 14, 237, 45, 246, 200, 57, 33, 37, 87, 245,
                243, 251, 12, 115, 224, 0, 56, 249, 224, 8, 246, 112, 23, 49, 120, 75
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
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 86, 1, 0, 0, 0, 103, 201, 160, 39, 1, 218, 188,
                23, 119, 82, 223, 174, 227, 238, 9, 40, 79, 51, 119, 45, 214, 139, 54, 187, 252,
                121, 41, 134, 86, 37, 236, 88, 25, 156, 185, 250, 162, 235, 67, 230, 99, 220, 200,
                15, 37, 178, 237, 93, 254, 62, 28, 131, 203, 15, 209, 112, 175, 176, 164, 218, 129,
                29, 169, 195, 228, 250, 122, 71, 8, 199, 182, 24, 53, 72, 42, 124, 84, 15, 253, 72,
                41, 184, 161, 219, 179, 135, 168, 169, 107, 7, 56, 0, 76, 15, 246, 195, 138, 76,
                14, 214, 209, 246, 178, 85, 138, 110, 240, 116, 174, 56, 154, 52
            ]
            .as_slice()
        )
    );
    assert_eq!(
        payloader.poll_packet().as_deref(),
        Some(
            [
                1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 86, 1, 0, 0, 0, 233, 115, 26, 93, 181, 180, 37,
                206, 47, 129, 189, 118, 3, 219, 106, 211, 95, 226, 170, 198, 163, 161, 92, 24, 9,
                245, 111, 182, 105, 155, 160, 250, 151, 162, 48, 120, 31, 91, 218, 100, 12, 184,
                78, 51, 17, 251, 181, 119, 115, 217, 177, 89, 106, 111, 71, 218, 72, 183, 42, 170,
                126, 201, 139, 40, 84, 236, 90, 102, 26, 182, 167, 204, 159, 61, 195, 251, 100, 39,
                142, 236, 195, 9, 49, 126, 152, 117, 14, 237, 45, 246, 200, 57, 33, 37, 87, 245,
                243, 251, 12, 115, 224, 0, 56, 249, 224, 8, 246, 112, 23, 49, 120, 75
            ]
            .as_slice()
        )
    );
    assert_eq!(
        payloader.poll_packet().as_deref(),
        Some(
            [
                2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 86, 1, 0, 0, 0, 202, 252, 231, 187, 132, 117, 171,
                214, 186, 70, 18, 161, 155, 142, 16, 150, 120, 245, 200, 130, 77, 4, 214, 207, 5,
                4, 123, 4, 239, 109, 192, 176, 16, 18, 50, 149, 122, 241, 115, 103, 82, 17, 224,
                119, 32, 201, 84, 98, 86, 53, 106, 0, 199, 146, 77, 24, 221, 166, 200, 238, 128,
                106, 180, 236, 64, 151, 65, 207, 179, 200, 137, 81, 202, 151, 226, 27, 58, 123, 32,
                33, 176, 239, 14, 85, 19, 109, 57, 204, 215, 164, 241, 218, 48, 130, 4, 197, 24,
                174, 28, 144, 16, 178, 61, 114, 108, 87, 97, 132, 228, 33, 231, 187
            ]
            .as_slice()
        )
    );
    assert_eq!(payloader.poll_packet().as_deref(), None);
}
