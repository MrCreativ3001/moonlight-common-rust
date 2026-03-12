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
        video::{ColorRange, ColorSpace, ServerCodecModeSupport, SupportedVideoFormats},
    },
};

// TODO: move more stuff out of c into mod, e.g. VideoDecoder, AudioDecoder
#[cfg(feature = "stream-c")]
pub mod c;

#[cfg(feature = "stream-proto")]
pub mod proto;

#[cfg(feature = "std")]
pub mod std;

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

impl Debug for AesKey {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "[RemoteInputAesKey]")
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
}

impl Debug for AesIv {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "[RemoteInputAesIv]")
    }
}

impl Deref for AesIv {
    type Target = u32;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// This contains technical details that are required for a stream to start.
///
/// Before starting a stream [MoonlightStreamConfig::adjust_for_server] should be called to support older servers.
///
/// References:
/// - Moonlight common c: https://github.com/moonlight-stream/moonlight-common-c/blob/62687809b1f7410c3db4be2527503a54ae408d70/src/Limelight.h#L524-L539
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
    /// AES encryption data for the remote input stream. This must be
    /// the same as what was passed as rikey and rikeyid
    /// in `/launch` and `/resume` requests.
    pub remote_input_aes_key: AesKey,
    /// AES encryption data for the remote input stream. This must be
    /// the same as what was passed as rikey and rikeyid
    /// in `/launch` and `/resume` requests.
    pub remote_input_aes_iv: AesIv,
    /// Apollo Extension
    ///
    /// See [ServerInfoEndpoint](crate::http::server_info::ServerInfoEndpoint)
    pub apollo_permissions: Option<ApolloPermissions>,
}

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
    pub supported_video_formats: SupportedVideoFormats,
    pub color_space: ColorSpace,
    pub color_range: ColorRange,
    pub local_audio_play_mode: bool,
    pub audio_config: AudioConfig,
    pub gamepads_attached: ActiveGamepads,
    pub gamepads_persist_after_disconnect: bool,
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
                .contains(!SupportedVideoFormats::MASK_H264)
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
    pub struct HostFeatures: u32 {
        const PEN_TOUCH_EVENTS = LI_FF_PEN_TOUCH_EVENTS;
        const CONTROLLER_TOUCH_EVENTS = LI_FF_CONTROLLER_TOUCH_EVENTS;
    }
}
