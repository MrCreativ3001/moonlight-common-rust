use ::std::{
    fmt::{self, Debug, Formatter},
    ops::Deref,
};

use bitflags::bitflags;
use num_derive::FromPrimitive;

use crate::{
    ServerVersion,
    http::server_info::ApolloPermissions,
    stream::{
        audio::AudioConfig,
        bindings::{
            ENCFLG_ALL, ENCFLG_AUDIO, ENCFLG_NONE, ENCFLG_VIDEO, LI_FF_CONTROLLER_TOUCH_EVENTS,
            LI_FF_PEN_TOUCH_EVENTS, STREAM_CFG_AUTO, STREAM_CFG_LOCAL, STREAM_CFG_REMOTE,
        },
        video::{ColorRange, ColorSpace, ServerCodecModeSupport, SupportedVideoFormats},
    },
};

// TODO: move more stuff out of c into mod, e.g. VideoDecoder, AudioDecoder
#[cfg(feature = "stream-c")]
pub mod c;

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

#[derive(Clone)]
pub struct AesKey(pub [u8; 16]);

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

pub struct AesIv(pub u32);

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
    pub gfe_version: String,
    /// The `ServerCodeModeSupport` of the server from the `/serverinfo` response
    ///
    /// See [ServerInfoEndpoint](crate::http::server_info::ServerInfoEndpoint)
    pub server_codec_mode_support: ServerCodecModeSupport,
    /// The rtsp session from the `/launch` or `/resume` response
    pub rtsp_session_url: String,
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

pub struct MoonlightStreamSettings {
    pub width: usize,
    pub height: usize,
    pub fps: usize,
    pub fps_x100: usize,
    pub bitrate: usize,
    pub packet_size: usize,
    pub encryption_flags: EncryptionFlags,
    pub streaming_remotely: StreamingConfig,
    pub supported_video_formats: SupportedVideoFormats,
    pub color_space: ColorSpace,
    pub color_range: ColorRange,
    pub audio_config: AudioConfig,
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
