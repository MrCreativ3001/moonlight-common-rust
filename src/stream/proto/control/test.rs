// TODO

use std::{
    error::Error,
    time::{Duration, Instant},
};

use crate::{
    ServerVersion, init_test,
    stream::{
        control::{KeyAction, KeyCode, KeyFlags, KeyModifiers, MouseButton, MouseButtonAction},
        proto::control::packet::{
            ControlPacket, ControlPacketConfig, ControlPacketType,
            ENCRYPTED_CONTROL_PACKET_AES_GCM_TAG_LENGTH, ENCRYPTED_CONTROL_PACKET_TYPE,
            EncryptedControlHeader, PacketDirection, TerminationReason,
        },
        video::{Primary, SunshineHdrMetadata},
    },
    test::init_test,
};

use crate::{
    crypto::disabled::DisabledCryptoBackend,
    stream::proto::{
        control::peer::{ControlHost, ControlHostConfig, ControlHostOutput},
        crypto::CryptoBackend,
    },
};

#[test]
fn client_server_peer() {
    let mut server = ControlHost::new(
        Instant::now(),
        ControlHostConfig {
            peer_count: 1,
            peer_channel_count: 1,
        },
        DisabledCryptoBackend,
    )
    .unwrap();

    let mut client = ControlHost::new(
        Instant::now(),
        ControlHostConfig {
            peer_count: 1,
            peer_channel_count: 1,
        },
        DisabledCryptoBackend,
    )
    .unwrap();

    // TODO: implement this
}

#[test]
fn test_encrypted_control_header_serialization() {
    let assert_eq_header =
        |deserialized: EncryptedControlHeader, serialized: [u8; EncryptedControlHeader::SIZE]| {
            let mut buffer = [0; EncryptedControlHeader::SIZE];
            deserialized.serialize(&mut buffer);

            assert_eq!(buffer, serialized);
            assert_eq!(EncryptedControlHeader::deserialize(&buffer), deserialized);
        };

    assert_eq_header(
        EncryptedControlHeader {
            ty: ENCRYPTED_CONTROL_PACKET_TYPE,
            len: 0x1234,
            sequence_number: 0xABCD,
            tag: [0x11; ENCRYPTED_CONTROL_PACKET_AES_GCM_TAG_LENGTH],
        },
        [
            // ty (LE)
            0x01, 0x00, // len (LE)
            0x34, 0x12, // sequence_number (LE, u32!)
            0xCD, 0xAB, 0x00, 0x00, // tag
            0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11,
            0x11, 0x11,
        ],
    );

    assert_eq_header(
        EncryptedControlHeader {
            ty: ENCRYPTED_CONTROL_PACKET_TYPE,
            len: 1,
            sequence_number: 2,
            tag: [
                0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF, 0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77,
                0x88, 0x99,
            ],
        },
        [
            // ty (LE)
            0x01, 0x00, // len (LE)
            0x01, 0x00, // sequence_number (LE, u32!)
            0x02, 0x00, 0x00, 0x00, // tag
            0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF, 0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77,
            0x88, 0x99,
        ],
    );
}

fn sunshine_gen_7_config() -> ControlPacketConfig {
    ControlPacketConfig::new(ServerVersion::new(7, 1, 431, -1), false).unwrap()
}

fn test_packet(
    direction: PacketDirection,
    config: ControlPacketConfig,
    expected_packet: ControlPacket,
    expected_bytes: &[u8],
) {
    assert_eq!(expected_packet.ty().direction(), direction);

    let mut bytes = [0; _];
    let len = expected_packet.serialize(&config, &mut bytes).unwrap();
    let bytes = &bytes[0..len];
    assert_eq!(bytes, expected_bytes, "Serialize: {:?}", expected_packet);

    let packet = ControlPacket::deserialize(direction, &config, bytes).unwrap();
    assert_eq!(
        packet, expected_packet,
        "Deserialize: {:?}",
        expected_packet
    );
}

#[test]
fn ping() {
    test_packet(
        PacketDirection::ServerBound,
        sunshine_gen_7_config(),
        ControlPacket::PeriodicPing,
        &[0, 2, 4, 0, 0, 0, 0, 0],
    );
}

#[test]
fn hdr_mode() {
    init_test!();

    test_packet(
        PacketDirection::ClientBound,
        sunshine_gen_7_config(),
        ControlPacket::HdrMode {
            enabled: false,
            sunshine: None,
        },
        &[14, 1, 1, 0, 0],
    );

    test_packet(
        PacketDirection::ClientBound,
        sunshine_gen_7_config(),
        ControlPacket::HdrMode {
            enabled: true,
            sunshine: None,
        },
        &[14, 1, 1, 0, 1],
    );
}
#[test]
fn hdr_mode_sunshine() {
    test_packet(
        PacketDirection::ClientBound,
        sunshine_gen_7_config(),
        ControlPacket::HdrMode {
            enabled: true,
            sunshine: Some(SunshineHdrMetadata {
                display_primaries: [
                    Primary { x: 34000, y: 16000 }, // Red
                    Primary { x: 13250, y: 34500 }, // Green
                    Primary { x: 7500, y: 3000 },   // Blue
                ],
                white_point: Primary { x: 15635, y: 16450 },
                max_display_luminance: 1000,
                min_display_luminance: 50,
                max_content_light_level: 1000,
                max_frame_average_light_level: 400,
                max_full_frame_luminance: 600,
            }),
        },
        &[
            14, 1, // Ty
            27, 0,    // Len
            0x01, // HDR enabled
            // Display Primaries
            0xD0, 0x84, // R.x = 34000
            0x80, 0x3E, // R.y = 16000
            0xC2, 0x33, // G.x = 13250
            0xC4, 0x86, // G.y = 34500
            0x4C, 0x1D, // B.x = 7500
            0xB8, 0x0B, // B.y = 3000
            // White point
            0x13, 0x3D, // x = 15635
            0x42, 0x40, // y = 16450
            // Luminance values
            0xE8, 0x03, // maxDisplayLuminance = 1000
            0x32, 0x00, // minDisplayLuminance = 50
            0xE8, 0x03, // maxContentLightLevel = 1000
            0x90, 0x01, // maxFrameAverageLightLevel = 400
            0x58, 0x02, // maxFullFrameLuminance = 600
        ],
    );
}

#[test]
fn request_idr() {
    test_packet(
        PacketDirection::ServerBound,
        sunshine_gen_7_config(),
        ControlPacket::RequestIdr,
        &[5, 3, 2, 0, 0, 0],
    );
}

#[test]
fn start_b() {
    test_packet(
        PacketDirection::ServerBound,
        sunshine_gen_7_config(),
        ControlPacket::StartB,
        &[7, 3, 1, 0, 0],
    );
}

#[test]
fn loss_stats() {
    test_packet(
        PacketDirection::ServerBound,
        sunshine_gen_7_config(),
        ControlPacket::LossStats {
            unknown1: 0,
            loss_report_interval_ms: 500,
            unknown2: 1000,
            last_good_frame: 1,
            unknown3: 0,
            unknown4: 0,
            unknown5: 0x14,
        },
        &[
            1, 2, // Type
            32, 0, // Length = 32
            0, 0, 0, 0, 244, 1, 0, 0, 232, 3, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            20, 0, 0, 0,
        ],
    );
}

#[test]
fn frame_stats() {
    init_test();

    test_packet(
        PacketDirection::ServerBound,
        sunshine_gen_7_config(),
        ControlPacket::FrameStats {},
        &[
            4, 2, // Type
            0, 0, // Length
        ],
    );
}

#[test]
fn frame_fec() {
    init_test();

    test_packet(
        PacketDirection::ServerBound,
        sunshine_gen_7_config(),
        ControlPacket::FrameFec {
            frame_index: 42,
            highest_received_sequence_number: 1200,
            next_contiguous_sequence_number: 1180,
            missing_packets_before_highest_received: 20,
            total_data_packets: 100,
            total_parity_packets: 10,
            received_data_packets: 95,
            received_parity_packets: 8,
            fec_percentage: 10,
            multi_fec_block_index: 0,
            multi_fec_block_count: 1,
        },
        &[
            2, 85, // Type
            21, 0x00, // Length = 21 (LE)
            0x00, 0x00, 0x00, 0x2A, // frame_index = 42 (BE)
            0x04, 0xB0, // highest_received_sequence_number = 1200
            0x04, 0x9C, // next_contiguous_sequence_number = 1180
            0x00, 0x14, // missing_packets_before_highest_received = 20
            0x00, 0x64, // total_data_packets = 100
            0x00, 0x0A, // total_parity_packets = 10
            0x00, 0x5F, // received_data_packets = 95
            0x00, 0x08, // received_parity_packets = 8
            0x0A, // fec_percentage = 10
            0x00, // multi_fec_block_index = 0
            0x01, // multi_fec_block_count = 1
        ],
    );
}

#[test]
fn invalidate_reference_frames() {
    test_packet(
        PacketDirection::ServerBound,
        sunshine_gen_7_config(),
        ControlPacket::InvalidateReferenceFrames {
            first_frame_index: 1,
            reserved1: 0,
            last_frame_index: 2,
            reserved2: [0, 0, 0],
        },
        &[
            0x01, 0x03, // Ty (0x0301 LE)
            24, 0x00, // Len
            1, 0, 0, 0, // first
            0, 0, 0, 0, // reserved1
            2, 0, 0, 0, // last
            0, 0, 0, 0, // reserved2[0]
            0, 0, 0, 0, // reserved2[1]
            0, 0, 0, 0, // reserved2[2]
        ],
    );
}

#[test]
fn long_term_reference_frame_acknowledgement() {
    test_packet(
        PacketDirection::ServerBound,
        sunshine_gen_7_config(),
        ControlPacket::LongTermReferenceFrameAcknowledgement {
            frame_index: 42,
            reserved: 0,
        },
        &[
            0x50, 0x03, // Ty (0x0350 LE)
            8, 0x00, // Len
            42, 0, 0, 0, 0, 0, 0, 0,
        ],
    );
}

#[test]
fn mouse_move_relative() {
    test_packet(
        PacketDirection::ServerBound,
        sunshine_gen_7_config(),
        ControlPacket::MouseMoveRelative {
            delta_x: 1,
            delta_y: 0,
        },
        &[
            0x06, 0x02, // Ty
            0x0c, 0x00, // Len
            0x00, 0x00, 0x00, 0x08, // Input Len
            0x07, 0x00, 0x00, 0x00, // Input Ty
            0x00, 0x01, // Delta X
            0x00, 0x00, // Delta Y
        ],
    );
}

#[test]
fn mouse_move_absolute() {
    test_packet(
        PacketDirection::ServerBound,
        sunshine_gen_7_config(),
        ControlPacket::MouseMoveAbsolute {
            x: 0,
            y: 1,
            unused: 0,
            reference_width: 1000,
            reference_height: 1000,
        },
        &[
            0x06, 0x02, // Ty
            0x12, 0x00, // Len
            0x00, 0x00, 0x00, 0x0e, // Input Len
            0x05, 0x00, 0x00, 0x00, // Input Ty
            0x00, 0x00, // X
            0x00, 0x01, // Y
            0x00, 0x00, // Unused
            3, 232, // Reference Width
            3, 232, // Reference Height
        ],
    );
}

#[test]
fn mouse_button() {
    test_packet(
        PacketDirection::ServerBound,
        sunshine_gen_7_config(),
        ControlPacket::MouseButton {
            action: MouseButtonAction::Press,
            button: MouseButton::Left,
        },
        &[
            0x06, 0x02, // Ty
            0x09, 0x00, // Len
            0x00, 0x00, 0x00, 0x05, // Input Len
            0x08, 0x00, 0x00, 0x00, // Mouse Action
            0x01, // Mouse Button
        ],
    );

    test_packet(
        PacketDirection::ServerBound,
        sunshine_gen_7_config(),
        ControlPacket::MouseButton {
            action: MouseButtonAction::Release,
            button: MouseButton::Left,
        },
        &[
            0x06, 0x02, // Ty
            0x09, 0x00, // Len
            0x00, 0x00, 0x00, 0x05, // Input Len
            0x09, 0x00, 0x00, 0x00, // Mouse Action
            0x01, // Mouse Button
        ],
    );
}

#[test]
fn mouse_scroll() {
    // See https://games-on-whales.github.io/wolf/stable/protocols/input-data.html#_mouse_scroll
    test_packet(
        PacketDirection::ServerBound,
        sunshine_gen_7_config(),
        ControlPacket::MouseScroll {
            scroll_amount_1: 2,
            scroll_amount_2: 2,
            zero: 0,
        },
        &[
            0x06, 0x02, // Type
            0x0e, 0x00, // Len
            0x00, 0x00, 0x00, 0xa, // Input Len
            0x0a, 0x00, 0x00, 0x00, // Input Type
            2, 0, // amount 1
            2, 0, // amount 2
            0x00, 0x00, // Zero
        ],
    );
}

#[test]
fn mouse_horizontal_scroll() {
    // See https://games-on-whales.github.io/wolf/stable/protocols/input-data.html#_mouse_horizontal_scroll
    test_packet(
        PacketDirection::ServerBound,
        sunshine_gen_7_config(),
        ControlPacket::MouseHorizontalScroll { scroll_amount: 10 },
        &[
            0x06, 0x02, // Type
            0x0a, 0x00, // Len
            0x00, 0x00, 0x00, 0x6, // Input Len
            0x01, 0x00, 0x00, 0x55, // Input Type
            10, 0x0, // amount
        ],
    );
}

#[test]
fn keyboard() {
    test_packet(
        PacketDirection::ServerBound,
        sunshine_gen_7_config(),
        ControlPacket::Keyboard {
            action: KeyAction::Down,
            flags: KeyFlags::empty(),
            key_code: KeyCode(0x41),
            modifier: KeyModifiers::CTRL,
            zero: 0,
        },
        &[
            0x06, 0x02, // Ty
            0x0e, 0x00, // Len
            0x00, 0x00, 0x00, 0x0a, // Input Len
            0x03, 0x00, 0x00, 0x00, // Key Action
            0x00, // Flags
            0x41, 0x00, // Key Code
            0x02, // Modifiers
            0x00, 0x00, // Zero
        ],
    );

    test_packet(
        PacketDirection::ServerBound,
        sunshine_gen_7_config(),
        ControlPacket::Keyboard {
            action: KeyAction::Up,
            flags: KeyFlags::SUNSHINE_NON_NORMALIZED,
            key_code: KeyCode(0x41),
            modifier: KeyModifiers::SHIFT,
            zero: 0,
        },
        &[
            0x06, 0x02, // Ty
            0x0e, 0x00, // Len
            0x00, 0x00, 0x00, 0x0a, // Input Len
            0x04, 0x00, 0x00, 0x00, // Key Action
            0x01, // Flags
            0x41, 0x00, // Key Code
            0x01, // Modifiers
            0x00, 0x00, // Zero
        ],
    );
}

#[test]
fn text_utf8() {
    // TODO: is this correctly implemented?

    test_packet(
        PacketDirection::ServerBound,
        sunshine_gen_7_config(),
        ControlPacket::text("hello").unwrap(),
        &[
            6, 2, // Type
            13, 0, // Length
            0, 0, 0, 9, // Input Len
            23, 0, 0, 0, // Input Type
            104, 101, 108, 108, 111, // Text as utf8
        ],
    );
}
#[test]
fn text_utf8_max() {
    let text = [b'X'; _];

    test_packet(
        PacketDirection::ServerBound,
        sunshine_gen_7_config(),
        ControlPacket::Text {
            text,
            text_len: text.len(),
        },
        &[
            6, 2, // Type
            40, 0, // Length
            0, 0, 0, 36, // Input Length
            23, 0, 0, 0, // Input Type
            88, 88, 88, 88, 88, 88, 88, 88, 88, 88, 88, 88, 88, 88, 88, 88, 88, 88, 88, 88, 88, 88,
            88, 88, 88, 88, 88, 88, 88, 88, 88, 88, // Text
        ],
    );
}

#[test]
fn touch() {
    todo!()
}

#[test]
fn pen() {
    todo!()
}

#[test]
fn controller_arrival() {
    todo!()
}

#[test]
fn controller_multi() {
    todo!()
}

#[test]
fn controller_motion() {
    todo!()
}

#[test]
fn controller_battery() {
    todo!()
}

#[test]
fn termination_long() {
    test_packet(
        PacketDirection::ClientBound,
        sunshine_gen_7_config(),
        ControlPacket::ServerTermination {
            reason: TerminationReason::GRACEFUL,
        },
        &[0, 1, 4, 0, 128, 3, 0, 35],
    );
}
#[test]
fn termination_short() {
    test_packet(
        PacketDirection::ClientBound,
        sunshine_gen_7_config(),
        ControlPacket::ServerTermination {
            reason: TerminationReason::Short(2),
        },
        &[0, 1, 2, 0, 0, 2],
    );
}
