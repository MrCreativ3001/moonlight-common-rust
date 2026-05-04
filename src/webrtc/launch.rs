use crate::{
    http::{
        FromQueryError, QueryBuilder, QueryBuilderError, QueryMap, QueryParam, Request,
        helper::{fmt_write_to_buffer, u32_to_str},
    },
    stream::{audio::AudioConfig, video::VideoFormats},
};
use std::fmt::Write as _;

#[derive(Debug, PartialEq)]
pub struct WebRtcLaunchRequest {
    /// The app the server should start.
    pub app_id: u32,
    /// The requested width of the stream.
    pub mode_width: u32,
    /// The requested height of the stream.
    pub mode_height: u32,
    /// The requested fps of the stream.
    pub mode_fps: u32,
    /// Request HDR.
    ///
    /// If the client enabled any [moonlight control stream](super::sdp::WebRtcClientFeatures), the server MUST send an [ControlPacket::HdrMode](crate::stream::proto::control::packet::ControlPacket::HdrMode) packet to inform the client.
    pub hdr: bool,
    /// If the audio should be played locally or only over the stream.
    pub local_audio_play_mode: bool,
    /// The bitrate of the stream.
    ///
    /// This is only a hint to the server.
    /// If the client supports other ways of adjusting bitrate the server is allowed to change the bitrate to improve the streaming experience.
    ///
    /// For Example:
    /// - [Transport Congestion Control](https://datatracker.ietf.org/doc/html/draft-holmer-rmcat-transport-wide-cc-extensions-01)
    /// - [Receiver Estimated Maximum Bandwidth](https://datatracker.ietf.org/doc/html/draft-alvestrand-rmcat-remb-03)
    ///
    /// If a client doesn't want that, it shouldn't advertise these capabilities.
    pub bitrate_kbps: u32,
    /// The preferred codecs.
    ///
    /// These codecs should be preferred.
    /// It could be possible that a codec is preferred that is not supported in sdp.
    /// The server MUST firstly check all supported codecs based on the sdp and should then see if it can select one preferred codec.
    /// After that the server should fallback to one codec supported in the sdp.
    pub preferred_codecs: VideoFormats,
    /// The preferred audio config.
    pub preferred_audio: AudioConfig,
    /// Moonlight Web Stream Extension
    ///
    /// Supported codecs of the session.
    ///
    /// This will overwrite the values given in the sdp.
    pub web_supported_codecs: Option<VideoFormats>,
    /// Moonlight Web Stream Extension
    ///
    /// Used to specify the host that should start the stream.
    pub web_host_id: Option<u32>,
}

impl Request for WebRtcLaunchRequest {
    fn append_query_params(
        &self,
        query_builder: &mut impl QueryBuilder,
    ) -> Result<(), QueryBuilderError> {
        let mut appid_buffer = [0u8; _];
        let appid = u32_to_str(self.app_id, &mut appid_buffer);
        query_builder.append(QueryParam {
            key: "appid",
            value: appid,
        })?;

        let mut mode_buffer = [0u8; (11 * 3) + 2];
        let mode = fmt_write_to_buffer(&mut mode_buffer, |writer| {
            write!(
                writer,
                "{}x{}x{}",
                self.mode_width, self.mode_height, self.mode_fps
            )
            .expect("write mode")
        });
        query_builder.append(QueryParam {
            key: "mode",
            value: mode,
        })?;

        let mut bitrate_buffer = [0; _];
        let bitrate = u32_to_str(self.bitrate_kbps, &mut bitrate_buffer);
        query_builder.append(QueryParam {
            key: "bitrate",
            value: bitrate,
        })?;

        if self.hdr {
            query_builder.append(QueryParam {
                key: "hdr",
                value: "1",
            })?;
        }

        query_builder.append(QueryParam {
            key: "localAudioPlayMode",
            value: if self.local_audio_play_mode { "1" } else { "0" },
        })?;

        let mut preferred_codec = [0u8; 11];
        let preferred_codec = u32_to_str(self.preferred_codecs.bits(), &mut preferred_codec);
        query_builder.append(QueryParam {
            key: "preferredCodec",
            value: preferred_codec,
        })?;

        let mut preferred_audio = [0u8; 11];
        let preferred_audio = u32_to_str(
            self.preferred_audio.to_surround_audio_info(),
            &mut preferred_audio,
        );
        query_builder.append(QueryParam {
            key: "preferredAudio",
            value: preferred_audio,
        })?;

        if let Some(web_supported_codecs) = self.web_supported_codecs {
            let mut web_supported_codecs_buffer = [0u8; _];
            let web_supported_codecs = u32_to_str(
                web_supported_codecs.bits(),
                &mut web_supported_codecs_buffer,
            );

            query_builder.append(QueryParam {
                key: "supportedCodecs",
                value: web_supported_codecs,
            })?;
        }

        if let Some(web_host) = self.web_host_id {
            let mut web_host_buffer = [0u8; _];
            let web_host_value = u32_to_str(web_host, &mut web_host_buffer);

            query_builder.append(QueryParam {
                key: "hostId",
                value: web_host_value,
            })?;
        }

        Ok(())
    }

    fn from_query_params<Q>(query_map: &Q) -> Result<Self, FromQueryError>
    where
        Q: QueryMap,
    {
        let app_id: u32 = query_map.get("appid")?.parse()?;

        let mode = query_map.get("mode")?;
        let mut mode_split = mode.split("x");
        let mode_width: u32 = mode_split
            .next()
            .ok_or(FromQueryError::Other(
                "Missing width in \"mode\"".to_string(),
            ))?
            .parse()?;
        let mode_height: u32 = mode_split
            .next()
            .ok_or(FromQueryError::Other(
                "Missing height in \"mode\"".to_string(),
            ))?
            .parse()?;
        let mode_fps: u32 = mode_split
            .next()
            .ok_or(FromQueryError::Other("Missing fps in \"mode\"".to_string()))?
            .parse()?;

        let bitrate = query_map.get("bitrate")?.parse::<u32>()?;

        let hdr = query_map.get("hdr").unwrap_or("0".into()) != "0";

        let preferred_codecs = query_map
            .get("preferredCodec")
            .ok()
            .map(|x| x.parse::<u32>())
            .transpose()?
            .map(VideoFormats::from_bits_retain)
            .unwrap_or(VideoFormats::empty());

        let preferred_audio = query_map
            .get("preferredAudio")
            .ok()
            .map(|x| x.parse())
            .transpose()?
            .unwrap_or(AudioConfig::STEREO.to_surround_audio_info());
        let preferred_audio = AudioConfig::from_surround_audio_info(preferred_audio);

        let local_audio_play_mode =
            query_map.get("localAudioPlayMode").unwrap_or("1".into()) != "0";

        let web_supported_codecs = query_map
            .get("supportedCodecs")
            .ok()
            .map(|x| x.parse::<u32>())
            .transpose()?
            .map(VideoFormats::from_bits_retain);

        let web_host = query_map
            .get("hostId")
            .ok()
            .map(|x| x.parse::<u32>())
            .transpose()?;

        Ok(Self {
            app_id,
            mode_width,
            mode_height,
            mode_fps,
            bitrate_kbps: bitrate,
            hdr,
            local_audio_play_mode,
            preferred_codecs,
            preferred_audio,
            web_supported_codecs,
            web_host_id: web_host,
        })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod test {
    use std::fmt::Debug;

    use crate::{
        http::Request,
        stream::{audio::AudioConfig, video::VideoFormats},
        webrtc::launch::WebRtcLaunchRequest,
    };

    pub fn assert_eq_request<R>(expected_request: R, expected_query: &str)
    where
        R: Request + PartialEq + Debug,
    {
        let mut str = String::new();
        expected_request.append_query_params(&mut str).unwrap();

        assert_eq!(str, expected_query);

        let request = R::from_query_params(&expected_query).unwrap();
        assert_eq!(request, expected_request);
    }

    #[test]
    fn webrtc_launch_request() {
        assert_eq_request(
            WebRtcLaunchRequest {
                app_id: 10,
                mode_width: 20,
                mode_height: 30,
                mode_fps: 40,
                hdr: true,
                local_audio_play_mode: true,
                bitrate_kbps: 67,
                preferred_codecs: VideoFormats::H265,
                preferred_audio: AudioConfig::STEREO,
                web_supported_codecs: None,
                web_host_id: None,
            },
            "appid=10&mode=20x30x40&bitrate=67&hdr=1&localAudioPlayMode=1&preferredCodec=256&preferredAudio=196610",
        );
    }

    #[test]
    fn web_webrtc_launch_request() {
        assert_eq_request(
            WebRtcLaunchRequest {
                app_id: 10,
                mode_width: 20,
                mode_height: 30,
                mode_fps: 40,
                hdr: false,
                local_audio_play_mode: true,
                bitrate_kbps: 67,
                preferred_codecs: VideoFormats::H265,
                preferred_audio: AudioConfig::STEREO,
                web_supported_codecs: Some(VideoFormats::H264),
                web_host_id: Some(69),
            },
            "appid=10&mode=20x30x40&bitrate=67&localAudioPlayMode=1&preferredCodec=256&preferredAudio=196610&supportedCodecs=1&hostId=69",
        );
    }
}
