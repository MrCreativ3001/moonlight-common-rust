//! This implements most functionality of the sdp generator for the client.
//!
//! References:
//! - https://github.com/moonlight-stream/moonlight-common-c/blob/b126e481a195fdc7152d211def17190e3434bcce/src/SdpGenerator.c#L255

use std::{net::IpAddr, time::Duration};

use bitflags::bitflags;

use crate::{
    ServerVersion,
    stream::{
        AudioConfig, ColorRange, ColorSpace, StreamingConfig, SupportedVideoFormats,
        proto::sdp::{Sdp, SdpMedia, SdpMediaType, SdpNetworkType, SdpOrigin, sdp_attr},
        video::VideoFormat,
    },
};

bitflags! {
    /// References:
    /// - https://github.com/moonlight-stream/moonlight-common-c/blob/b126e481a195fdc7152d211def17190e3434bcce/src/Limelight-internal.h#L87
    #[derive(Debug, Clone, Copy, PartialEq)]
    pub struct MoonlightFeatureFlags: u32 {
        // ML_FF_FEC_STATUS 0x01 // Client sends SS_FRAME_FEC_STATUS for frame losses
        const FEC_STATUS = 0x01;
        // ML_FF_SESSION_ID_V1 0x02 // Client supports X-SS-Ping-Payload and X-SS-Connect-Data
        const SESSION_ID_V1 = 0x02;
    }
}

bitflags! {
    /// References:
    /// - https://github.com/moonlight-stream/moonlight-common-c/blob/b126e481a195fdc7152d211def17190e3434bcce/src/Limelight-internal.h#L47
    #[derive(Debug, Clone, Copy, PartialEq)]
    pub struct SunshineEncryptionFlags: u32 {
        const CONTROL_V2 = 0x01;
        const VIDEO = 0x02;
        const AUDIO = 0x04;
    }
}

// TODO: bitrate adjustments: https://github.com/moonlight-stream/moonlight-common-c/blob/b126e481a195fdc7152d211def17190e3434bcce/src/SdpGenerator.c#L336

#[derive(Debug, Default, Clone)]
pub enum ChromaSamplingType {
    #[default]
    Auto,
    YUV444,
}

bitflags! {
    #[derive(Debug, Clone)]
    pub struct NvFeatureFlags: u32 {
        const BASE             = 0x07;
        const AUDIO_ENCRYPTION = 0x20;
        const RI_ENCRYPTION    = 0x80;
    }
}

// TODO: this might be interesting: https://github.com/moonlight-stream/moonlight-common-c/blob/435bc6a5a4852c90cfb037de1378c0334ed36d8e/src/SdpGenerator.c#L556-L563

#[derive(Debug, Default, Clone)]
pub struct ClientSdp {
    /// Required: Default is 14
    /// https://github.com/moonlight-stream/moonlight-common-c/blob/b126e481a195fdc7152d211def17190e3434bcce/src/SdpGenerator.c#L546
    pub rtsp_client_version: Option<usize>,
    /// Required: Use the target ip
    /// https://github.com/moonlight-stream/moonlight-common-c/blob/b126e481a195fdc7152d211def17190e3434bcce/src/SdpGenerator.c#L546
    pub target_ip: Option<IpAddr>,
    pub client_viewport_width: Option<u32>,
    pub client_viewport_height: Option<u32>,
    pub max_fps: Option<u32>,
    /// default is 1024
    pub packet_size: Option<u32>,
    /// default is 4
    pub rate_control_mode: Option<u32>,
    /// default is 7000ms
    pub timeout_length: Option<Duration>,
    /// default is 0
    pub frames_with_invalid_ref_threshold: Option<u32>,
    // TODO: differentiate between them, version docs / check?: https://github.com/moonlight-stream/moonlight-common-c/blob/b126e481a195fdc7152d211def17190e3434bcce/src/SdpGenerator.c#L359
    // We don't support dynamic bitrate scaling properly (it tends to bounce between min and max and never
    // settle on the optimal bitrate if it's somewhere in the middle), so we'll just latch the bitrate
    // to the requested value.
    pub initial_bitrate_kbps: Option<u32>,
    pub initial_peak_bitrate_kbps: Option<u32>,
    pub vqos_maximum_bitrate_kbps: Option<u32>,
    pub vqos_minium_bitrate_kbps: Option<u32>,
    /// Only enabled when AppVersionQuad[0] < 5 && Streaming Remote
    /// default is 4
    pub average_bitrate: Option<u32>,
    /// Only enabled when AppVersionQuad[0] < 5 && Streaming Remote
    /// default is 4
    pub peak_bitrate: Option<u32>,
    pub maximum_bitrate: Option<u32>,
    pub minimum_bitrate: Option<u32>,
    /// AppVersionQuad[0] >= 5
    /// Sunshine extension: Send the configured bitrate to Sunshine hosts, so they can adjust for dynamic FEC percentage
    pub sunshine_configured_bitrate_kbps: Option<u32>,
    /// default is true
    pub enable_fec: Option<bool>,
    /// default is 5000
    pub video_quality_score_update_time: Option<u32>,
    /// default is 0, but if streaming local it's 5
    /// https://github.com/moonlight-stream/moonlight-common-c/blob/b126e481a195fdc7152d211def17190e3434bcce/src/SdpGenerator.c#L399
    /// If the remote host is local (RFC 1918), enable QoS tagging for our traffic. Windows qWave
    /// will disable it if the host is off-link, *however* Windows may get it wrong in cases where
    /// the host is directly connected to the Internet without a NAT. In this case, it may send DSCP
    /// marked traffic off-link and it could lead to black holes due to misconfigured ISP hardware
    /// or CPE. For this reason, we only enable it in cases where it looks like it will work.
    ///
    /// Even though IPv6 hardware should be much less likely to have this issue, we can't tell
    /// if our address is a NAT64 synthesized IPv6 address or true end-to-end IPv6. If it's the
    /// former, it may have the same problem as other IPv4 traffic.
    pub vqos_traffic_type: Option<u32>,
    /// default is 0, but if streaming local it's 4
    /// https://github.com/moonlight-stream/moonlight-common-c/blob/b126e481a195fdc7152d211def17190e3434bcce/src/SdpGenerator.c#L399
    pub aqos_traffic_type: Option<u32>,
    /// This seems to be just "0.0.0.0" to get around some audio restrictions:
    /// https://github.com/moonlight-stream/moonlight-common-c/blob/b126e481a195fdc7152d211def17190e3434bcce/src/RtspConnection.c#L945
    /// Gen 3: ???
    /// Gen 4: ???
    pub server_address: Option<String>,
    /// This seems to be vec![0x41514141, 0x41514141, 0x41514141, 0x41514141]
    pub transfer_protocol: Vec<u32>,
    /// This seems to be vec![0x42414141, 0x42514141, 0x42514141, 0x42514141]
    /// Seems to exist two times
    pub rate_control_mode2: Vec<u32>,
    /// This seems to be vec![14083]
    pub bw_flags: Vec<u32>,
    /// This seems to be vec![0, 0, 0, 0]
    pub video_qos_max_consecutive_drops: Vec<u32>,
    // -- Gen 3 options: https://github.com/moonlight-stream/moonlight-common-c/blob/b126e481a195fdc7152d211def17190e3434bcce/src/SdpGenerator.c#L124
    /// This seems to be the value 0x42774141
    /// These are all NvFeatureFlags turned on (BASE | AUDIO_ENCRYPTION | RI_ENCRYPTION)
    // -- Gen 5 options: https://github.com/moonlight-stream/moonlight-common-c/blob/b126e481a195fdc7152d211def17190e3434bcce/src/SdpGenerator.c#L180
    /// APP_VERSION_AT_LEAST(7, 1, 431)
    /// RI encryption is always enabled
    pub features_flags: Option<NvFeatureFlags>,
    /// Ask for the encrypted control protocol to ensure remote input will be encrypted.
    /// This used to be done via separate RI encryption, but now it is all or nothing.
    /// if APP_VERSION_AT_LEAST(7, 1, 431) = 13 else 1
    pub use_reliable_udp: Option<usize>,
    /// APP_VERSION_AT_LEAST(7, 1, 431)
    /// Require at least 2 FEC packets for small frames. If a frame has fewer data shards
    /// than would generate 2 FEC shards, it will increase the FEC percentage for that frame
    /// above the set value (even going as high as 200% FEC to generate 2 FEC shards from a
    /// 1 data shard frame).
    pub min_required_fec_packets: Option<usize>,
    /// APP_VERSION_AT_LEAST(7, 1, 431)
    /// BLL-FEC appears to adjust dynamically based on the loss rate and instantaneous bitrate
    /// of each frame, however we can't dynamically control it from our side yet. As a result,
    /// the effective FEC amount is significantly lower (single digit percentages for many
    /// large frames) and the result is worse performance during packet loss. Disabling BLL-FEC
    /// results in GFE 3.26 falling back to the legacy FEC method as we would like.
    pub enable_bll_fec: Option<bool>,
    /// APP_VERSION_AT_SMALLER_EQ(7, 1, 431)
    /// true
    pub use_control_channel: Option<bool>,
    /// APP_VERSION_AT_SMALLER_EQ(7, 1, 431)
    /// When streaming 4K, lower FEC levels to reduce stream overhead
    /// https://github.com/moonlight-stream/moonlight-common-c/blob/b126e481a195fdc7152d211def17190e3434bcce/src/SdpGenerator.c#L224C9-L224C73
    pub fec_repair_percent: Option<u32>,
    /// true if APP_VERSION_AT_LEAST(7, 1, 446) && (StreamConfig.width < 720 || StreamConfig.height < 540)
    /// We enable DRC with a static DRC table for very low resoutions on GFE 3.26 to work around
    /// a bug that causes nvstreamer.exe to crash due to failing to populate a list of valid resolutions.
    ///
    /// Despite the fact that the DRC table doesn't include our target streaming resolution, we still
    /// seem to stream at the target resolution, presumably because we don't send control data to tell
    /// the host otherwise.
    pub enable_drc: Option<bool>,
    /// Use if drc_enable is true: set to 2
    /// https://github.com/moonlight-stream/moonlight-common-c/blob/b126e481a195fdc7152d211def17190e3434bcce/src/SdpGenerator.c#L241
    pub drc_table_type: Option<u32>,
    /// Recovery mode can cause the FEC percentage to change mid-frame, which
    /// breaks many assumptions in RTP FEC queue.
    pub enable_recovery_mode: Option<bool>,
    /// Use slicing for increased performance on some decoders
    /// If not using slicing, we should request 1 slice per frame
    pub slices_per_frame: Option<u32>,
    pub vqos_bit_stream_format: Option<u32>,
    pub client_support_hevc: Option<bool>,
    /// This disables split frame encode on GFE 3.10 which seems to produce broken
    /// HEVC output at 1080p60 (full of artifacts even on the SHIELD itself, go figure).
    /// It now appears to work fine on GFE 3.14.1.
    pub video_encoder_feature_setting: Option<u32>,
    /// Enable HDR
    pub dynamic_range_mode: Option<bool>,
    /// If the decoder supports reference frame invalidation, that indicates it also supports
    /// the maximum number of reference frames allowed by the codec. Even if we can't use RFI
    /// due to lack of host support, we can still allow the host to pick a number of reference
    /// frames greater than 1 to improve encoding efficiency.
    ///
    /// Restrict the video stream to 1 reference frame if we're not using
    /// reference frame invalidation. This helps to improve compatibility with
    /// some decoders that don't like the default of having 16 reference frames.
    pub max_num_reference_frames: Option<u32>,
    pub client_refresh_rate_x100: Option<u32>,
    pub audio_surround_num_channels: Option<u32>,
    pub audio_surround_channel_mask: Option<u32>,
    pub audio_surround_enable: Option<bool>,
    /// Enabled when more than 2 channels, this also influences the opus config
    /// https://github.com/moonlight-stream/moonlight-common-c/blob/435bc6a5a4852c90cfb037de1378c0334ed36d8e/src/AudioStream.c#L428-L438
    /// https://github.com/moonlight-stream/moonlight-common-c/blob/b126e481a195fdc7152d211def17190e3434bcce/src/SdpGenerator.c#L492-L530
    pub audio_surround_quality: Option<bool>,
    pub audio_packet_duration: Option<Duration>,
    pub encoder_csc_mode: Option<(ColorSpace, ColorRange)>,
    /// The video port which is in the mline of the sdp
    pub video_port: Option<u16>,
    /// Sunshine extension: moonlight feature flags
    pub sunshine_moonlight_feature_flags: Option<MoonlightFeatureFlags>,
    /// Sunshine extension: only use when advertise, New-style control stream encryption is low overhead
    /// Important for video: Adjust the video packet size to account for encryption overhead, https://github.com/moonlight-stream/moonlight-common-c/blob/b126e481a195fdc7152d211def17190e3434bcce/src/SdpGenerator.c#L322C8-L322C71
    pub sunshine_encryption: Option<SunshineEncryptionFlags>,
    /// Sunshine extension: Enable YUV444 if requested
    pub sunshine_chroma_sampling_type: Option<ChromaSamplingType>,
}

impl ClientSdp {
    /// Creates a sdp like moonlight common c
    ///
    /// Some other values are changed which this function won't do:
    /// - adjust packet size: https://github.com/moonlight-stream/moonlight-common-c/blob/b126e481a195fdc7152d211def17190e3434bcce/src/SdpGenerator.c#L322
    /// - adjust bitrate: https://github.com/moonlight-stream/moonlight-common-c/blob/b126e481a195fdc7152d211def17190e3434bcce/src/SdpGenerator.c#L336-L354
    /// - When streaming 4K, lower FEC levels to reduce stream overhead: https://github.com/moonlight-stream/moonlight-common-c/blob/b126e481a195fdc7152d211def17190e3434bcce/src/SdpGenerator.c#L224C9-L230
    /// - Slices per frame: https://github.com/moonlight-stream/moonlight-common-c/blob/b126e481a195fdc7152d211def17190e3434bcce/src/SdpGenerator.c#L424-L431
    /// - Max Num Reference Frames: https://github.com/moonlight-stream/moonlight-common-c/blob/b126e481a195fdc7152d211def17190e3434bcce/src/SdpGenerator.c#L462-L474
    ///
    /// Other hints:
    /// - server_address: look in the struct
    /// - audio_surround_quality: look in the struct
    pub fn new(
        streaming_remotely: StreamingConfig,
        server_version: ServerVersion,
        target_ip: IpAddr,
        moonlight_feature_flags: MoonlightFeatureFlags,
        sunshine_encryption_flags: SunshineEncryptionFlags,
        negotiated_video_format: VideoFormat,
        width: u32,
        height: u32,
        max_fps: u32,
        fps_x100: u32,
        packet_size: u32,
        bitrate: u32,
        server_address: String,
        rtsp_port: u16,
        fec_repair_percent: u32,
        audio_config: AudioConfig,
        high_quality_audio: bool,
        slices_per_frame: u32,
        max_num_reference_frames: u32,
        color_space: ColorSpace,
        color_range: ColorRange,
        video_port: u16,
    ) -> Self {
        let mut sdp = Self::default();

        assert_ne!(streaming_remotely, StreamingConfig::Auto);

        if server_version.is_sunshine_like() {
            sdp.sunshine_moonlight_feature_flags = Some(moonlight_feature_flags);

            sdp.sunshine_encryption = Some(sunshine_encryption_flags);

            if negotiated_video_format.contained_in(SupportedVideoFormats::MASK_YUV444) {
                sdp.sunshine_chroma_sampling_type = Some(ChromaSamplingType::YUV444);
            } else {
                sdp.sunshine_chroma_sampling_type = Some(ChromaSamplingType::Auto);
            }
        } else {
            assert_eq!(moonlight_feature_flags, MoonlightFeatureFlags::empty());
            assert_eq!(sunshine_encryption_flags, SunshineEncryptionFlags::empty());
        }

        sdp.client_viewport_width = Some(width);
        sdp.client_viewport_height = Some(height);
        sdp.max_fps = Some(max_fps);

        sdp.packet_size = Some(packet_size);
        sdp.rate_control_mode = Some(4);

        sdp.timeout_length = Some(Duration::from_secs(7));
        sdp.frames_with_invalid_ref_threshold = Some(0);

        // -- Server version dependant
        if server_version.major >= 5 {
            sdp.initial_bitrate_kbps = Some(bitrate);
            sdp.initial_peak_bitrate_kbps = Some(bitrate);

            sdp.vqos_minium_bitrate_kbps = Some(bitrate);
            sdp.vqos_maximum_bitrate_kbps = Some(bitrate);

            if server_version.is_sunshine_like() {
                sdp.sunshine_configured_bitrate_kbps = Some(bitrate);
            }
        } else {
            if streaming_remotely == StreamingConfig::Remote {
                sdp.average_bitrate = Some(4);
                sdp.peak_bitrate = Some(4);
            }

            sdp.minimum_bitrate = Some(bitrate);
            sdp.maximum_bitrate = Some(bitrate);
        }

        sdp.enable_fec = Some(true);
        sdp.video_quality_score_update_time = Some(5000);

        if streaming_remotely == StreamingConfig::Local {
            sdp.vqos_traffic_type = Some(5);
            sdp.aqos_traffic_type = Some(4);
        } else {
            sdp.vqos_traffic_type = Some(0);
            sdp.aqos_traffic_type = Some(0);
        }

        if server_version.major == 3 {
            sdp.server_address = Some(server_address.to_string());

            sdp.features_flags = Some(
                NvFeatureFlags::BASE
                    | NvFeatureFlags::AUDIO_ENCRYPTION
                    | NvFeatureFlags::RI_ENCRYPTION,
            );

            sdp.transfer_protocol = vec![0x41514141, 0x41514141, 0x41514141, 0x41514141];
            sdp.rate_control_mode2 = vec![0x42414141, 0x42514141, 0x42514141, 0x42514141];

            sdp.bw_flags = vec![14083];

            sdp.video_qos_max_consecutive_drops = vec![0, 0, 0, 0];
        } else if server_version.major == 4 {
            sdp.server_address = Some(format!("rtsp://{}:{}", server_address, rtsp_port));
        } else {
            if server_version >= ServerVersion::new(7, 1, 431, 0) {
                let mut nv_feature_flags = NvFeatureFlags::BASE | NvFeatureFlags::RI_ENCRYPTION;
                // Enable audio encryption if the client opted in or the host required it
                if sunshine_encryption_flags.contains(SunshineEncryptionFlags::AUDIO) {
                    nv_feature_flags |= NvFeatureFlags::AUDIO_ENCRYPTION;
                }

                sdp.features_flags = Some(nv_feature_flags);

                // Ask for the encrypted control protocol to ensure remote input will be encrypted.
                // This used to be done via separate RI encryption, but now it is all or nothing.
                sdp.use_reliable_udp = Some(13);

                // Require at least 2 FEC packets for small frames. If a frame has fewer data shards
                // than would generate 2 FEC shards, it will increase the FEC percentage for that frame
                // above the set value (even going as high as 200% FEC to generate 2 FEC shards from a
                // 1 data shard frame).
                sdp.min_required_fec_packets = Some(2);

                // BLL-FEC appears to adjust dynamically based on the loss rate and instantaneous bitrate
                // of each frame, however we can't dynamically control it from our side yet. As a result,
                // the effective FEC amount is significantly lower (single digit percentages for many
                // large frames) and the result is worse performance during packet loss. Disabling BLL-FEC
                // results in GFE 3.26 falling back to the legacy FEC method as we would like.
                sdp.enable_bll_fec = Some(false);
            } else {
                // We want to use the new ENet connections for control and input
                sdp.use_reliable_udp = Some(1);

                sdp.use_control_channel = Some(true);

                sdp.fec_repair_percent = Some(fec_repair_percent);
            }

            if server_version >= ServerVersion::new(7, 1, 446, 0) && (width < 720 || height < 540) {
                // We enable DRC with a static DRC table for very low resoutions on GFE 3.26 to work around
                // a bug that causes nvstreamer.exe to crash due to failing to populate a list of valid resolutions.
                //
                // Despite the fact that the DRC table doesn't include our target streaming resolution, we still
                // seem to stream at the target resolution, presumably because we don't send control data to tell
                // the host otherwise.
                sdp.enable_drc = Some(true);
                sdp.drc_table_type = Some(2);
            } else {
                // Disable dynamic resolution switching
                sdp.enable_drc = Some(false);
            }

            // Recovery mode can cause the FEC percentage to change mid-frame, which
            // breaks many assumptions in RTP FEC queue.
            sdp.enable_recovery_mode = Some(false);
        }

        // -- Video
        if server_version.major >= 4 {
            sdp.slices_per_frame = Some(slices_per_frame);

            if negotiated_video_format.contained_in(SupportedVideoFormats::MASK_AV1) {
                sdp.vqos_bit_stream_format = Some(2);
            } else if negotiated_video_format.contained_in(SupportedVideoFormats::MASK_H265) {
                sdp.client_support_hevc = Some(true);
                sdp.vqos_bit_stream_format = Some(1);

                if server_version < ServerVersion::new(7, 1, 408, 0) {
                    // This disables split frame encode on GFE 3.10 which seems to produce broken
                    // HEVC output at 1080p60 (full of artifacts even on the SHIELD itself, go figure).
                    // It now appears to work fine on GFE 3.14.1.

                    // TODO: log? https://github.com/moonlight-stream/moonlight-common-c/blob/b126e481a195fdc7152d211def17190e3434bcce/src/SdpGenerator.c#L444
                    sdp.video_encoder_feature_setting = Some(0);
                }
            } else {
                sdp.client_support_hevc = Some(false);
                sdp.vqos_bit_stream_format = Some(0);
            }
        }

        if server_version.major >= 7 {
            // Enable HDR if request
            if negotiated_video_format.contained_in(SupportedVideoFormats::MASK_10BIT) {
                sdp.dynamic_range_mode = Some(true);
            } else {
                sdp.dynamic_range_mode = Some(false);
            }

            sdp.max_num_reference_frames = Some(max_num_reference_frames);

            sdp.client_refresh_rate_x100 = Some(fps_x100);

            sdp.audio_surround_num_channels = Some(audio_config.channel_count);
            sdp.audio_surround_channel_mask = Some(audio_config.channel_mask);
            sdp.audio_surround_enable = Some(audio_config.channel_count > 2);

            // TODO: audio stuff: https://github.com/moonlight-stream/moonlight-common-c/blob/b126e481a195fdc7152d211def17190e3434bcce/src/SdpGenerator.c#L492-L530
            sdp.audio_surround_quality = Some(high_quality_audio);

            sdp.encoder_csc_mode = Some((color_space, color_range));
        }
        // TODO: packeet duration: https://github.com/moonlight-stream/moonlight-common-c/blob/435bc6a5a4852c90cfb037de1378c0334ed36d8e/src/SdpGenerator.c#L522
        sdp.audio_packet_duration = Some(Duration::from_millis(5));

        sdp.rtsp_client_version = Some(14);
        sdp.target_ip = Some(target_ip);

        sdp.video_port = Some(video_port);

        sdp
    }

    #[allow(clippy::field_reassign_with_default)]
    pub fn into_sdp(self) -> Sdp {
        let mut sdp = Sdp::default();

        // TODO: move sdpversion, SdpOrigin into this ClientSdp struct
        sdp.version = Some(0);
        if let Some(rtsp_client_version) = self.rtsp_client_version
            && let Some(target_ip) = self.target_ip
        {
            sdp.origin = Some(SdpOrigin {
                username: "android".into(),
                session_id: 0,
                session_version: rtsp_client_version,
                network_type: SdpNetworkType::In,
                ip: target_ip,
            });
        }
        sdp.session = Some("NVIDIA Streaming Client".into());

        if let Some(sunshine_moonlight_feature_flags) = self.sunshine_moonlight_feature_flags {
            sdp.attributes.push(sdp_attr(
                "x-ml-general.featureFlags",
                sunshine_moonlight_feature_flags.bits(),
            ));
        }

        if let Some(sunshine_encryption) = self.sunshine_encryption {
            sdp.attributes.push(sdp_attr(
                "x-ss-general.encryptionEnabled",
                sunshine_encryption.bits(),
            ))
        }

        if let Some(sunshine_chroma_sampling_type) = self.sunshine_chroma_sampling_type {
            let value = match sunshine_chroma_sampling_type {
                ChromaSamplingType::Auto => "0",
                ChromaSamplingType::YUV444 => "1",
            };

            sdp.attributes
                .push(sdp_attr("x-ss-video[0].chromaSamplingType", value));
        }

        if let Some(width) = self.client_viewport_width {
            sdp.attributes
                .push(sdp_attr("x-nv-video[0].clientViewportWd", width));
        }
        if let Some(height) = self.client_viewport_height {
            sdp.attributes
                .push(sdp_attr("x-nv-video[0].clientViewportHt", height));
        }

        if let Some(fps) = self.max_fps {
            sdp.attributes.push(sdp_attr("x-nv-video[0].maxFPS", fps));
        }

        if let Some(packet_size) = self.packet_size {
            sdp.attributes
                .push(sdp_attr("x-nv-video[0].packetSize", packet_size));
        }

        if let Some(timeout_length) = self.timeout_length {
            sdp.attributes.push(sdp_attr(
                "x-nv-video[0].timeoutLengthMs",
                timeout_length.as_millis(),
            ));
        }

        if let Some(frames_with_invalid_ref_threshold) = self.frames_with_invalid_ref_threshold {
            sdp.attributes.push(sdp_attr(
                "x-nv-video[0].framesWithInvalidRefThreshold",
                frames_with_invalid_ref_threshold,
            ));
        }

        if let Some(rate_control_mode) = self.rate_control_mode {
            sdp.attributes
                .push(sdp_attr("x-nv-video[0].rateControlMode", rate_control_mode));
        }

        if let Some(initial_bitrate_kbps) = self.initial_bitrate_kbps {
            sdp.attributes.push(sdp_attr(
                "x-nv-video[0].initialBitrateKbps",
                initial_bitrate_kbps,
            ));
        }

        if let Some(initial_peak_bitrate_kbps) = self.initial_peak_bitrate_kbps {
            sdp.attributes.push(sdp_attr(
                "x-nv-video[0].initialPeakBitrateKbps",
                initial_peak_bitrate_kbps,
            ));
        }

        if let Some(vqos_minimum_bitrate_kbps) = self.vqos_minium_bitrate_kbps {
            sdp.attributes.push(sdp_attr(
                "x-nv-vqos[0].bw.minimumBitrateKbps",
                vqos_minimum_bitrate_kbps,
            ));
        }

        if let Some(vqos_maximum_bitrate_kbps) = self.vqos_maximum_bitrate_kbps {
            sdp.attributes.push(sdp_attr(
                "x-nv-vqos[0].bw.maximumBitrateKbps",
                vqos_maximum_bitrate_kbps,
            ));
        }

        if let Some(average_bitrate_kbps) = self.average_bitrate {
            sdp.attributes.push(sdp_attr(
                "x-nv-video[0].averageBitrate",
                average_bitrate_kbps,
            ));
        }

        if let Some(peak_bitrate) = self.peak_bitrate {
            sdp.attributes
                .push(sdp_attr("x-nv-video[0].peakBitrate", peak_bitrate));
        }

        if let Some(sunshine_configured_bitrate_kbps) = self.sunshine_configured_bitrate_kbps {
            sdp.attributes.push(sdp_attr(
                "x-ml-video.configuredBitrateKbps",
                sunshine_configured_bitrate_kbps,
            ));
        }

        if let Some(minimum_bitrate) = self.minimum_bitrate {
            sdp.attributes
                .push(sdp_attr("x-nv-vqos[0].bw.minimumBitrate", minimum_bitrate));
        }

        if let Some(maximum_bitrate) = self.maximum_bitrate {
            sdp.attributes
                .push(sdp_attr("x-nv-vqos[0].bw.maximumBitrate", maximum_bitrate));
        }

        if let Some(enable_fec) = self.enable_fec {
            sdp.attributes.push(sdp_attr(
                "x-nv-vqos[0].fec.enable",
                if enable_fec { 1 } else { 0 },
            ));
        }

        if let Some(video_quality_score_update_time) = self.video_quality_score_update_time {
            sdp.attributes.push(sdp_attr(
                "x-nv-vqos[0].videoQualityScoreUpdateTime",
                video_quality_score_update_time,
            ));
        }

        if let Some(vqos_traffic_type) = self.vqos_traffic_type {
            sdp.attributes
                .push(sdp_attr("x-nv-vqos[0].qosTrafficType", vqos_traffic_type));
        }

        if let Some(aqos_traffic_type) = self.aqos_traffic_type {
            sdp.attributes
                .push(sdp_attr("x-nv-aqos.qosTrafficType", aqos_traffic_type));
        }

        if let Some(server_address) = self.server_address {
            sdp.attributes
                .push(sdp_attr("x-nv-general.serverAddress", server_address));
        }

        for (i, value) in self.transfer_protocol.iter().enumerate() {
            // TODO: stack allocate?
            sdp.attributes.push(sdp_attr(
                format!("x-nv-video[{}].transferProtocol", i),
                value,
            ));
        }

        for (i, value) in self.rate_control_mode2.iter().enumerate() {
            // TODO: stack allocate?
            sdp.attributes.push(sdp_attr(
                format!("x-nv-video[{}].rateControlMode", i),
                value,
            ));
        }

        for (i, value) in self.bw_flags.iter().enumerate() {
            // TODO: stack allocate?
            sdp.attributes
                .push(sdp_attr(format!("x-nv-video[{}].bw.flags", i), value));
        }

        for (i, value) in self.video_qos_max_consecutive_drops.iter().enumerate() {
            // TODO: stack allocate?
            sdp.attributes.push(sdp_attr(
                format!("x-nv-vqos[{}].videoQosMaxConsecutiveDrops", i),
                value,
            ));
        }

        if let Some(use_reliable_udp) = self.use_reliable_udp {
            sdp.attributes
                .push(sdp_attr("x-nv-general.useReliableUdp", use_reliable_udp));
        }

        if let Some(min_required_fec_packets) = self.min_required_fec_packets {
            sdp.attributes.push(sdp_attr(
                "x-nv-vqos[0].fec.minRequiredFecPackets",
                min_required_fec_packets,
            ));
        }

        if let Some(enable_bll_fec) = self.enable_bll_fec {
            sdp.attributes.push(sdp_attr(
                "x-nv-vqos[0].bllFec.enable",
                if enable_bll_fec { "1" } else { "0" },
            ));
        }

        if let Some(use_control_channel) = self.use_control_channel {
            sdp.attributes.push(sdp_attr(
                "x-nv-ri.useControlChannel",
                if use_control_channel { "1" } else { "0" },
            ));
        }

        if let Some(fec_repair_percent) = self.fec_repair_percent {
            sdp.attributes.push(sdp_attr(
                "x-nv-vqos[0].fec.repairPercent",
                fec_repair_percent,
            ));
        }

        if let Some(enable_drc) = self.enable_drc {
            sdp.attributes.push(sdp_attr(
                "x-nv-vqos[0].drc.enable",
                if enable_drc { "1" } else { "0" },
            ));
        }

        if let Some(drc_table_type) = self.drc_table_type {
            sdp.attributes
                .push(sdp_attr("x-nv-vqos[0].drc.tableType", drc_table_type));
        }

        if let Some(enable_recovery_mode) = self.enable_recovery_mode {
            sdp.attributes.push(sdp_attr(
                "x-nv-general.enableRecoveryMode",
                enable_recovery_mode,
            ));
        }

        if let Some(slices_per_frame) = self.slices_per_frame {
            sdp.attributes.push(sdp_attr(
                "x-nv-video[0].videoEncoderSlicesPerFrame",
                slices_per_frame,
            ));
        }

        if let Some(vqos_bit_stream_format) = self.vqos_bit_stream_format {
            sdp.attributes.push(sdp_attr(
                "x-nv-vqos[0].bitStreamFormat",
                vqos_bit_stream_format,
            ));
        }

        if let Some(client_support_hevc) = self.client_support_hevc {
            sdp.attributes.push(sdp_attr(
                "x-nv-clientSupportHevc",
                if client_support_hevc { "1" } else { "0" },
            ));
        }

        if let Some(video_encoder_feature_settings) = self.video_encoder_feature_setting {
            sdp.attributes.push(sdp_attr(
                "x-nv-video[0].encoderFeatureSetting",
                video_encoder_feature_settings,
            ));
        }

        if let Some(dynamic_range_mode) = self.dynamic_range_mode {
            sdp.attributes.push(sdp_attr(
                "x-nv-video[0].dynamicRangeMode",
                if dynamic_range_mode { 1 } else { 0 },
            ));
        }

        if let Some(max_num_reference_frames) = self.max_num_reference_frames {
            sdp.attributes.push(sdp_attr(
                "x-nv-video[0].maxNumReferenceFrames",
                max_num_reference_frames,
            ));
        }

        if let Some(client_refresh_rate_x100) = self.client_refresh_rate_x100 {
            sdp.attributes.push(sdp_attr(
                "x-nv-video[0].clientRefreshRateX100",
                client_refresh_rate_x100,
            ));
        }

        if let Some(audio_surround_num_channels) = self.audio_surround_num_channels {
            sdp.attributes.push(sdp_attr(
                "x-nv-audio.surround.numChannels",
                audio_surround_num_channels,
            ));
        }

        if let Some(audio_surround_channel_mask) = self.audio_surround_channel_mask {
            sdp.attributes.push(sdp_attr(
                "x-nv-audio.surround.channelMask",
                audio_surround_channel_mask,
            ));
        }

        if let Some(audio_surround_enable) = self.audio_surround_enable {
            sdp.attributes.push(sdp_attr(
                "x-nv-audio.surround.enable",
                if audio_surround_enable { "1" } else { "0" },
            ));
        }

        if let Some(audio_surround_quality) = self.audio_surround_quality {
            sdp.attributes.push(sdp_attr(
                "x-nv-audio.surround.AudioQuality",
                if audio_surround_quality { 1 } else { 0 },
            ));
        }

        if let Some(audio_packet_duration) = self.audio_packet_duration {
            sdp.attributes.push(sdp_attr(
                "x-nv-aqos.packetDuration",
                audio_packet_duration.as_millis(),
            ));
        }

        if let Some((color_space, color_range)) = self.encoder_csc_mode {
            sdp.attributes.push(sdp_attr(
                "x-nv-video[0].encoderCscMode",
                (((color_space as u8) << 1) | color_range as u8) as usize,
            ));
        }

        if let Some(video_port) = self.video_port {
            sdp.media.push(SdpMedia {
                media_type: SdpMediaType::Video,
                port: video_port,
            });
        }

        sdp
    }
}
