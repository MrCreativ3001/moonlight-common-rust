use crate::stream::{
    RawHostFeatures, VideoFormats,
    audio::OpusMultistreamConfig,
    proto::{
        rtsp::moonlight::ParseMoonlightRtspResponseError,
        sdp::{Sdp, client::SunshineEncryptionFlags},
    },
};

#[derive(Debug, Default)]
pub struct ServerSdp {
    /// Sample rate is always 48 KHz
    /// Stereo doesn't have any surround-params elements in the RTSP data
    ///
    /// References:
    /// - <https://github.com/moonlight-stream/moonlight-common-c/blob/b126e481a195fdc7152d211def17190e3434bcce/src/RtspConnection.c#L734>
    pub audio_surround_params: Vec<OpusMultistreamConfig>,
    /// The supported video formats based on the sdp.
    /// You should not only rely on this value alone and should also use the [serverCodecModeSupport](crate::http::server_info::ServerInfoResponse::server_codec_mode_support) from the serverinfo.
    ///
    /// References:
    /// - <https://github.com/moonlight-stream/moonlight-common-c/blob/b126e481a195fdc7152d211def17190e3434bcce/src/RtspConnection.c#L1076-L1122>
    pub video_formats: VideoFormats,
    pub video_reference_frame_invalidation: Option<bool>,
    /// Sunshine extension: <https://github.com/moonlight-stream/moonlight-common-c/blob/b126e481a195fdc7152d211def17190e3434bcce/src/RtspConnection.c#L1130>
    pub sunshine_feature_flags: Option<RawHostFeatures>,
    /// Sunshine extension: <https://github.com/moonlight-stream/moonlight-common-c/blob/b126e481a195fdc7152d211def17190e3434bcce/src/RtspConnection.c#L1135>
    pub sunshine_encryption_supported: Option<SunshineEncryptionFlags>,
    /// Sunshine extension: <https://github.com/moonlight-stream/moonlight-common-c/blob/b126e481a195fdc7152d211def17190e3434bcce/src/RtspConnection.c#L1139>
    pub sunshine_encryption_requested: Option<SunshineEncryptionFlags>,
}

impl ServerSdp {
    pub fn parse(sdp: Sdp) -> Result<Self, ParseMoonlightRtspResponseError> {
        let mut parsed = ServerSdp {
            // H264 is support on every server by default
            // See https://github.com/moonlight-stream/moonlight-common-c/blob/b126e481a195fdc7152d211def17190e3434bcce/src/RtspConnection.c#L1115
            video_formats: VideoFormats::MASK_H264,
            ..Default::default()
        };

        for attribute in sdp.attributes {
            if attribute.key == "x-ss-general.featureFlags"
                && let Some(value) = attribute.value
            {
                parsed.sunshine_feature_flags =
                    Some(RawHostFeatures::from_bits_retain(value.parse()?));
            } else if attribute.key == "x-ss-general.encryptionSupported"
                && let Some(value) = attribute.value
            {
                parsed.sunshine_encryption_supported =
                    Some(SunshineEncryptionFlags::from_bits_truncate(value.parse()?));
            } else if attribute.key == "x-ss-general.encryptionRequested"
                && let Some(value) = attribute.value
            {
                parsed.sunshine_encryption_requested =
                    Some(SunshineEncryptionFlags::from_bits_truncate(value.parse()?));
            } else if attribute.key == "sprop-parameter-sets=AAAAAU" {
                // The RTSP DESCRIBE reply will contain a collection of SDP media attributes that
                // describe the various supported video stream formats and include the SPS, PPS,
                // and VPS (if applicable). We will use this information to determine whether the
                // server can support HEVC. For some reason, they still set the MIME type of the HEVC
                // format to H264, so we can't just look for the HEVC MIME type. What we'll do instead is
                // look for the base 64 encoded VPS NALU prefix that is unique to the HEVC bitstream.

                // See
                // https://github.com/LizardByte/Sunshine/blob/7228c2553c393739c3387d5a152c9b255be2328f/src/rtsp.cpp#L793
                // https://github.com/moonlight-stream/moonlight-common-c/blob/b126e481a195fdc7152d211def17190e3434bcce/src/RtspConnection.c#L1091
                parsed.video_formats |= VideoFormats::MASK_H265;
            } else if attribute.key == "AV1/90000" {
                // If the server supports AV1
                // See
                // https://github.com/LizardByte/Sunshine/blob/7228c2553c393739c3387d5a152c9b255be2328f/src/rtsp.cpp#L797
                // https://github.com/moonlight-stream/moonlight-common-c/blob/b126e481a195fdc7152d211def17190e3434bcce/src/RtspConnection.c#L1076C103-L1076C112
                parsed.video_formats |= VideoFormats::MASK_AV1;
            } else if attribute.key == "x-nv-video[0].refPicInvalidation" {
                parsed.video_reference_frame_invalidation = Some(true);
            } else if attribute.key == "fmtp"
                && let Some(value) = attribute.value
                && let Some(value) = value.strip_prefix("97 surround-params=")
                && let Ok(value) = value.parse::<u64>()
            {
                // fmtp line looks like this "a=fmtp:97 surround-params=%d"
                // https://github.com/moonlight-stream/moonlight-common-c/blob/b126e481a195fdc7152d211def17190e3434bcce/src/RtspConnection.c#L759

                parsed
                    .audio_surround_params
                    .push(OpusMultistreamConfig::from_surround_param(value));
            }
        }

        Ok(parsed)
    }
}
