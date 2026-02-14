use crate::{
    stream::proto::{
        rtsp::moonlight::ParseMoonlightRtspResponseError,
        sdp::{Sdp, client::SunshineEncryptionFlags},
    },
    stream::{HostFeatures, SupportedVideoFormats},
};

// TODO: avoid using c

#[derive(Debug, Default)]
pub struct ServerSdp {
    // TODO: parse audio data correctly:
    /// Sample rate is always 48 KHz
    /// Stereo doesn't have any surround-params elements in the RTSP data
    /// https://github.com/moonlight-stream/moonlight-common-c/blob/b126e481a195fdc7152d211def17190e3434bcce/src/RtspConnection.c#L734
    // TODO: is this AudioConfig?
    pub audio_surround_params: Vec<usize>,
    pub video_formats: Option<SupportedVideoFormats>,
    pub video_reference_frame_invalidation: Option<bool>,
    /// Sunshine extension: https://github.com/moonlight-stream/moonlight-common-c/blob/b126e481a195fdc7152d211def17190e3434bcce/src/RtspConnection.c#L1130
    pub sunshine_feature_flags: Option<HostFeatures>,
    // TODO: what does encryption flag 0x04 mean?
    /// Sunshine extension: https://github.com/moonlight-stream/moonlight-common-c/blob/b126e481a195fdc7152d211def17190e3434bcce/src/RtspConnection.c#L1135
    pub sunshine_encryption_supported: Option<SunshineEncryptionFlags>,
    /// Sunshine extension: https://github.com/moonlight-stream/moonlight-common-c/blob/b126e481a195fdc7152d211def17190e3434bcce/src/RtspConnection.c#L1139
    pub sunshine_encryption_requested: Option<SunshineEncryptionFlags>,
}

impl ServerSdp {
    // TODO: maybe a different error?
    pub fn parse(sdp: Sdp) -> Result<Self, ParseMoonlightRtspResponseError> {
        let mut parsed = ServerSdp::default();

        // TODO: move this into sdp moonlight
        for attribute in sdp.attributes {
            if attribute.key == "x-ss-general.featureFlags"
                && let Some(value) = attribute.value
            {
                parsed.sunshine_feature_flags =
                    Some(HostFeatures::from_bits_truncate(value.parse()?));
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

                // TODO: where is this in the sdp?
                // TODO: why do they do this? https://github.com/moonlight-stream/moonlight-common-c/blob/b126e481a195fdc7152d211def17190e3434bcce/src/RtspConnection.c#L1091
            } else if attribute.key == "AV1/90000" {
                // TODO: Av1? https://github.com/moonlight-stream/moonlight-common-c/blob/b126e481a195fdc7152d211def17190e3434bcce/src/RtspConnection.c#L1076C103-L1076C112
                // TODO: where is this in the sdp?
            } else if attribute.key == "x-nv-video[0].refPicInvalidation" {
                // TODO: where is this in the sdp?
                parsed.video_reference_frame_invalidation = Some(true);
            } else if attribute.key == "fmtp"
                && let Some(value) = attribute.value
                && let Some(value) = value.strip_prefix("97 surround-params=")
                && let Ok(value) = value.parse::<usize>()
            {
                // fmtp line looks like this "a=fmtp:97 surround-params=%d"
                // https://github.com/moonlight-stream/moonlight-common-c/blob/b126e481a195fdc7152d211def17190e3434bcce/src/RtspConnection.c#L759

                // TODO: maybe warn about failed parsing?
                parsed.audio_surround_params.push(value);
            }
        }

        Ok(parsed)
    }
}
