use crate::{
    ServerVersion,
    stream::{
        EncryptionFlags,
        proto::{
            MoonlightStreamSetupError, negotiate_sunshine_encryption, negotiated_video_packet_size,
            sdp::client::SunshineEncryptionFlags as Flags,
        },
    },
};

fn sunshine() -> ServerVersion {
    ServerVersion::new(7, 1, 431, -1)
}

/// What a Sunshine with `lan_encryption_mode = 2` / `wan_encryption_mode = 2` advertises
fn mandatory() -> (Flags, Flags) {
    (
        Flags::CONTROL_V2 | Flags::VIDEO | Flags::AUDIO,
        Flags::CONTROL_V2 | Flags::VIDEO | Flags::AUDIO,
    )
}

/// What a Sunshine with `lan_encryption_mode = 1` advertises
fn optional() -> (Flags, Flags) {
    (
        Flags::CONTROL_V2,
        Flags::CONTROL_V2 | Flags::VIDEO | Flags::AUDIO,
    )
}

/// What a Sunshine with `lan_encryption_mode = 0` advertises
fn disabled() -> (Flags, Flags) {
    (Flags::CONTROL_V2, Flags::CONTROL_V2 | Flags::AUDIO)
}

fn negotiate(client: EncryptionFlags, (requested, supported): (Flags, Flags)) -> Flags {
    negotiate_sunshine_encryption(sunshine(), client, requested, supported)
}

#[test]
fn mandatory_host_encrypts_everything_the_client_wants() {
    assert_eq!(
        negotiate(EncryptionFlags::AUDIO | EncryptionFlags::VIDEO, mandatory()),
        Flags::CONTROL_V2 | Flags::VIDEO | Flags::AUDIO
    );
}

#[test]
fn mandatory_host_overrules_a_client_opt_out() {
    assert_eq!(
        negotiate(EncryptionFlags::empty(), mandatory()),
        Flags::CONTROL_V2 | Flags::VIDEO | Flags::AUDIO
    );
}

#[test]
fn optional_host_follows_the_client() {
    assert!(negotiate(EncryptionFlags::VIDEO, optional()).contains(Flags::VIDEO));
    assert!(!negotiate(EncryptionFlags::AUDIO, optional()).contains(Flags::VIDEO));
}

#[test]
fn host_without_video_encryption_isnt_asked_for_it() {
    assert!(!negotiate(EncryptionFlags::VIDEO, disabled()).contains(Flags::VIDEO));
}

#[test]
fn only_sunshine_hosts_negotiate_encryption() {
    let (requested, supported) = mandatory();

    assert_eq!(
        negotiate_sunshine_encryption(
            ServerVersion::new(7, 1, 431, 0),
            EncryptionFlags::VIDEO,
            requested,
            supported
        ),
        Flags::empty()
    );
}

#[test]
fn packet_size_makes_room_for_the_encryption_header() {
    let encrypted = Flags::CONTROL_V2 | Flags::VIDEO;
    let plain = Flags::CONTROL_V2 | Flags::AUDIO;

    // Same as moonlight-common-c: the datagram size doesn't change with encryption
    assert_eq!(negotiated_video_packet_size(1024, encrypted), Ok(992));
    assert_eq!(negotiated_video_packet_size(1184, encrypted), Ok(1152));
    assert_eq!(negotiated_video_packet_size(1024, plain), Ok(1024));
}

#[test]
fn packet_size_is_rounded_down_to_the_fec_chunk_size() {
    assert_eq!(negotiated_video_packet_size(1039, Flags::VIDEO), Ok(992));
    assert_eq!(negotiated_video_packet_size(1039, Flags::empty()), Ok(1024));
}

#[test]
fn packet_size_too_small_is_an_error() {
    for packet_size in [0, 16, 48] {
        assert_eq!(
            negotiated_video_packet_size(packet_size, Flags::VIDEO),
            Err(MoonlightStreamSetupError::VideoPacketSizeTooSmall { packet_size })
        );
    }
    assert_eq!(negotiated_video_packet_size(64, Flags::VIDEO), Ok(32));
}
