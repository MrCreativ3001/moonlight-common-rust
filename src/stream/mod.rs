use ::std::{
    fmt::{self, Debug, Formatter},
    ops::Deref,
};

use bitflags::bitflags;
use num_derive::FromPrimitive;

use crate::{
    ServerVersion,
    high::StreamConfigError,
    http::{pair::PairingCryptoBackend, server_info::ApolloPermissions},
    stream::{
        audio::AudioConfig,
        bindings::{
            ENCFLG_ALL, ENCFLG_AUDIO, ENCFLG_NONE, ENCFLG_VIDEO, LI_FF_CONTROLLER_TOUCH_EVENTS,
            LI_FF_PEN_TOUCH_EVENTS, STREAM_CFG_AUTO, STREAM_CFG_LOCAL, STREAM_CFG_REMOTE,
        },
        control::ActiveGamepads,
        video::{ColorRange, ColorSpace, ServerCodecModeSupport, VideoFormats},
    },
};

#[cfg(feature = "stream-c")]
pub mod c;

#[cfg(feature = "stream-proto")]
pub mod proto;

#[cfg(feature = "std")]
pub mod std;

#[cfg(feature = "tokio")]
pub mod tokio;

// Common implementation details

pub mod audio;
pub mod connection;
pub mod control;
pub mod debug;
pub mod video;

#[allow(unused)]
mod bindings;

#[derive(Clone, Copy, PartialEq)]
pub struct AesKey(pub [u8; 16]);

impl AesKey {
    pub fn new_random<Crypto>(crypto_backend: &Crypto) -> Result<Self, Crypto::Error>
    where
        Crypto: PairingCryptoBackend,
    {
        let mut key = [0; _];
        crypto_backend.random_bytes(&mut key)?;

        Ok(Self(key))
    }
}

impl Deref for AesKey {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[cfg(not(feature = "__tracing_sensitive"))]
impl Debug for AesKey {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "[AesKey]")
    }
}

#[cfg(feature = "__tracing_sensitive")]
impl Debug for AesKey {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "AesKey({:?})", self.0)
    }
}

#[derive(Clone, Copy, PartialEq)]
pub struct AesIv(pub u32);

impl AesIv {
    pub fn new_random<Crypto>(crypto_backend: &Crypto) -> Result<Self, Crypto::Error>
    where
        Crypto: PairingCryptoBackend,
    {
        let mut iv = [0; 4];
        crypto_backend.random_bytes(&mut iv)?;

        Ok(Self(u32::from_le_bytes(iv)))
    }

    /// Useful for passing the [AesIv] into functions from the [CryptoBackend](proto::crypto::CryptoBackend)
    pub fn to_array(self) -> [u8; 4] {
        self.0.to_le_bytes()
    }
}

#[cfg(not(feature = "__tracing_sensitive"))]
impl Debug for AesIv {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "[AesIv]")
    }
}

#[cfg(feature = "__tracing_sensitive")]
impl Debug for AesIv {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "AesIv({:?})", self.0)
    }
}

impl Deref for AesIv {
    type Target = u32;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Debug, Clone, Copy)]
pub struct SunshineEncryption {
    /// AES encryption data for the remote input stream. This must be
    /// the same as what was passed as rikey and rikeyid
    /// in `/launch` and `/resume` requests.
    pub aes_key: AesKey,
    /// AES encryption data for the remote input stream. This must be
    /// the same as what was passed as rikey and rikeyid
    /// in `/launch` and `/resume` requests.
    pub aes_iv: AesIv,
}

/// This contains technical details that are required for a stream to start.
///
/// References:
/// - Moonlight common c: <https://github.com/moonlight-stream/moonlight-common-c/blob/62687809b1f7410c3db4be2527503a54ae408d70/src/Limelight.h#L524-L539>
#[derive(Debug)]
pub struct MoonlightStreamConfig {
    /// The address of the server
    pub address: String,
    /// The `appversion` of the server from the `/serverinfo` response
    ///
    /// See [ServerInfoEndpoint](crate::http::server_info::ServerInfoEndpoint)
    pub version: ServerVersion,
    /// The `GfeVersion` of the server from the `/serverinfo` response
    ///
    /// See [ServerInfoEndpoint](crate::http::server_info::ServerInfoEndpoint)
    pub gfe_version: Option<String>,
    /// The `ServerCodeModeSupport` of the server from the `/serverinfo` response
    ///
    /// See [ServerInfoEndpoint](crate::http::server_info::ServerInfoEndpoint)
    pub server_codec_mode_support: ServerCodecModeSupport,
    /// The rtsp session from the `/launch` or `/resume` response
    pub rtsp_session_url: Option<String>,
    /// See [SunshineEncryption]
    pub encryption: SunshineEncryption,
    /// Apollo Extension
    ///
    /// See [ServerInfoEndpoint](crate::http::server_info::ServerInfoEndpoint)
    pub apollo_permissions: Option<ApolloPermissions>,
    /// Foundation Extension
    ///
    /// If the foundation microphone should be enabled.
    ///
    /// Only enable this if the server supports foundation's microphone protocol.
    ///
    /// See also:
    /// - [Foundation Microphone Module](crate::stream::proto::microphone::foundation)
    /// - [Foundation Microphone Payloader](crate::stream::proto::microphone::foundation::payloader::FoundationMicPayloader)
    ///
    /// References:
    /// - <https://github.com/Yundi339/moonlight-common-c/blob/f59424a9f7ad86f2b6278a4e2b07fb2902d8b090/src/Limelight.h#L105-L106>
    // TODO: find out a specific version (Foundation Sunshine Version in the serverinfo response?) at which this is supported
    pub foundation_enable_mic: bool,
}

///
/// Before starting a stream [MoonlightStreamConfig::adjust_for_server] should be called to support older servers.
///
#[derive(Debug, Clone)]
pub struct MoonlightStreamSettings {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub fps_x100: u32,
    pub bitrate: u32,
    pub packet_size: u32,
    pub encryption_flags: EncryptionFlags,
    pub streaming_remotely: StreamingConfig,
    pub sops: bool,
    pub hdr: bool,
    pub supported_video_formats: VideoFormats,
    pub color_space: ColorSpace,
    pub color_range: ColorRange,
    pub local_audio_play_mode: bool,
    pub audio_config: AudioConfig,
    pub gamepads_attached: ActiveGamepads,
    pub gamepads_persist_after_disconnect: bool,
    /// This will enable microphone support, if the server supports it.
    pub enable_mic: bool,
}

impl MoonlightStreamSettings {
    /// Some servers don't support certain settings.
    /// This will try to make the settings compatible with older servers.
    ///
    /// If this doesn't work it'll fail.
    pub fn adjust_for_server(
        &mut self,
        version: ServerVersion,
        gfe_version: &str,
        server_codec_mode_support: ServerCodecModeSupport,
    ) -> Result<(), StreamConfigError> {
        let supports_hdr = Self::is_hdr_supported(server_codec_mode_support);

        if self.hdr && !supports_hdr {
            return Err(StreamConfigError::NotSupportedHdr);
        }

        self.check_resolution_supported(version, gfe_version, server_codec_mode_support)?;

        if version.is_nvidia_software() {
            // Using an FPS value over 60 causes SOPS to default to 720p60,
            // so force it to 0 to ensure the correct resolution is set. We
            // used to use 60 here but that locked the frame rate to 60 FPS
            // on GFE 3.20.3. We don't need this hack for Sunshine.
            if self.fps > 60 {
                self.fps = 0;
            }

            if self.should_disable_sops(version) {
                self.sops = false;
            }
        }

        Ok(())
    }

    fn is_hdr_supported(server_codec_mode_support: ServerCodecModeSupport) -> bool {
        server_codec_mode_support.contains(ServerCodecModeSupport::HEVC_MAIN10)
            || server_codec_mode_support.contains(ServerCodecModeSupport::AV1_MAIN10)
    }
    fn is_4k_supported(
        version: ServerVersion,
        server_codec_mode_support: ServerCodecModeSupport,
    ) -> bool {
        server_codec_mode_support.contains(ServerCodecModeSupport::HEVC_MAIN10)
            || !version.is_nvidia_software()
    }
    fn is_4k_supported_gfe(gfe_version: &str) -> bool {
        !gfe_version.starts_with("2.")
    }

    fn check_resolution_supported(
        &self,
        version: ServerVersion,
        gfe_version: &str,
        server_codec_mode_support: ServerCodecModeSupport,
    ) -> Result<(), StreamConfigError> {
        let resolution_above_4k = self.width > 4096 || self.height > 4096;
        let supports_4k = Self::is_4k_supported(version, server_codec_mode_support);
        let supports_4k_gfe = Self::is_4k_supported_gfe(gfe_version);

        if resolution_above_4k && !supports_4k {
            return Err(StreamConfigError::NotSupported4k);
        } else if resolution_above_4k
            && self
                .supported_video_formats
                .contains(!VideoFormats::MASK_H264)
        {
            return Err(StreamConfigError::NotSupported4kCodecMissing);
        } else if self.height > 2160 && supports_4k_gfe {
            return Err(StreamConfigError::NotSupported4kUpdateGfe);
        }

        Ok(())
    }

    pub fn should_disable_sops(&self, version: ServerVersion) -> bool {
        // Using an unsupported resolution (not 720p, 1080p, or 4K) causes
        // GFE to force SOPS to 720p60. This is fine for < 720p resolutions like
        // 360p or 480p, but it is not ideal for 1440p and other resolutions.
        // When we detect an unsupported resolution, disable SOPS unless it's under 720p.
        // FIXME: Detect support resolutions using the serverinfo response, not a hardcoded list
        const NVIDIA_SUPPORTED_RESOLUTIONS: &[(u32, u32)] =
            &[(1280, 720), (1920, 1080), (3840, 2160)];

        let is_nvidia = version.is_nvidia_software();

        !NVIDIA_SUPPORTED_RESOLUTIONS.contains(&(self.width, self.height)) && is_nvidia
    }
}

bitflags! {
    #[derive(Debug, Clone, Copy)]
    pub struct EncryptionFlags: u32 {
        const AUDIO = ENCFLG_AUDIO;
        const VIDEO  = ENCFLG_VIDEO;

        // Other extensions
        // TODO: this might need to be seperated into two structs: SunshineEncryption, FoundationSunshineEncryption

        /// Foundation Extension
        ///
        /// If microphone encryption should be enabled.
        ///
        /// References:
        /// - Mic PR: https://github.com/Yundi339/moonlight-common-c/blob/f59424a9f7ad86f2b6278a4e2b07fb2902d8b090/src/Limelight.h#L36
        const FOUNDATION_MICROPHONE = 0x04;

        const NONE = ENCFLG_NONE;
        const ALL = ENCFLG_ALL;
    }
}

#[repr(u32)]
#[derive(Debug, Clone, Copy, FromPrimitive, PartialEq)]
pub enum StreamingConfig {
    Local = STREAM_CFG_LOCAL,
    Remote = STREAM_CFG_REMOTE,
    Auto = STREAM_CFG_AUTO,
}

bitflags! {
    #[derive(Debug, Clone)]
    pub struct RawHostFeatures: u32 {
        const PEN_TOUCH_EVENTS = LI_FF_PEN_TOUCH_EVENTS;
        const CONTROLLER_TOUCH_EVENTS = LI_FF_CONTROLLER_TOUCH_EVENTS;
    }
}

impl RawHostFeatures {
    pub fn into_host_features(self, server_version: ServerVersion) -> HostFeatures {
        let extensions = HostProtocolExtensions {
            sunshine_pen: self.contains(RawHostFeatures::PEN_TOUCH_EVENTS),
            sunshine_touch: self.contains(RawHostFeatures::PEN_TOUCH_EVENTS),
            sunshine_controller_touch: self.contains(RawHostFeatures::CONTROLLER_TOUCH_EVENTS),
            ..Default::default()
        };

        HostFeatures {
            // See https://github.com/moonlight-stream/moonlight-common-c/blob/7b026e77be62175104640e7e722b758df6d3d0d7/src/InputStream.c#L1021-L1038
            controller_limit: if server_version.is_sunshine_like() {
                16
            } else {
                4
            },
            pen: extensions.sunshine_pen,
            touch: extensions.sunshine_touch,
            controller_touch: extensions.sunshine_controller_touch,
            extensions,
            ..Default::default()
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct HostProtocolExtensions {
    /// Sunshine Extension
    ///
    /// If the host supports pen touch events.
    ///
    /// References:
    /// - <https://github.com/moonlight-stream/moonlight-common-c/blob/7b026e77be62175104640e7e722b758df6d3d0d7/src/Limelight.h#L1003>
    pub sunshine_pen: bool,
    /// Sunshine Extension
    ///
    /// If the host supports general touch events.
    ///
    /// References:
    /// - <https://github.com/moonlight-stream/moonlight-common-c/blob/7b026e77be62175104640e7e722b758df6d3d0d7/src/Limelight.h#L1003>
    pub sunshine_touch: bool,
    /// Sunshine Extension
    ///
    /// If the host supports controller touch events.
    ///
    /// References:
    /// - <https://github.com/moonlight-stream/moonlight-common-c/blob/7b026e77be62175104640e7e722b758df6d3d0d7/src/Limelight.h#L1004>
    pub sunshine_controller_touch: bool,
    /// Apollo Extension
    ///
    /// The permissions from Apollo.
    ///
    /// Use this to query if specific apollo packets are supported or not and if you have permissions to send these packets or also other packets.
    ///
    /// See also:
    /// - [ApolloPermissions]
    ///
    /// References:
    /// - packet definitions: <https://github.com/ClassicOldSong/Apollo/blob/dd99a82247de72ad16b8804ae85822ddc8222c3a/src/stream.cpp#L73-L74>
    /// - server commands: <https://github.com/ClassicOldSong/Apollo/blob/dd99a82247de72ad16b8804ae85822ddc8222c3a/src/stream.cpp#L1004-L1035>
    /// - set / get clipboard: <https://github.com/ClassicOldSong/Apollo/blob/dd99a82247de72ad16b8804ae85822ddc8222c3a/src/nvhttp.cpp#L1526-L1634>
    pub apollo_permissions: Option<ApolloPermissions>,
    /// Foundation Sunshine Extension
    ///
    /// References:
    /// - <https://github.com/AlkaidLab/foundation-sunshine/blob/c2d8de3650a2ddf65281b6694e623c865a603831/src/stream.cpp#L88-L89>
    pub foundation_microphone: bool,
    // TODO: https://github.com/AlkaidLab/foundation-sunshine/blob/c2d8de3650a2ddf65281b6694e623c865a603831/src/stream.cpp#L90-L91
}

#[derive(Debug, Clone, PartialEq)]
pub struct HostFeatures {
    /// The controller limit of the host.
    pub controller_limit: usize,
    /// If the host supports pen events
    pub pen: bool,
    /// If the host supports touch events
    pub touch: bool,
    /// If the host supports controller touch events
    pub controller_touch: bool,
    /// If the host supports any microphone protocol.
    pub microphone: bool,
    /// More details about the extensions this host supports
    pub extensions: HostProtocolExtensions,
}

impl Default for HostFeatures {
    fn default() -> Self {
        HostFeatures {
            controller_limit: 4,
            controller_touch: false,
            pen: false,
            touch: false,
            microphone: false,
            extensions: HostProtocolExtensions::default(),
        }
    }
}
