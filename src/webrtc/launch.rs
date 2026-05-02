use crate::{
    http::{
        FromQueryError, QueryBuilder, QueryBuilderError, QueryMap, QueryParam, Request,
        helper::{fmt_write_to_buffer, i32_to_str, u32_to_str},
    },
    stream::audio::AudioConfig,
};
use std::fmt::Write as _;

pub struct WebRtcLaunchRequest {
    pub app_id: u32,
    pub mode_width: u32,
    pub mode_height: u32,
    pub mode_fps: u32,
    pub hdr: bool,
    pub surround_audio_info: AudioConfig,
    pub local_audio_play_mode: bool,
    pub gamepads_attached_mask: i32,
    pub gamepads_persist_after_disconnect: bool,
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

        query_builder.append(QueryParam {
            key: "additionalStates",
            value: "1",
        })?;

        if self.hdr {
            query_builder.append(QueryParam {
                key: "hdrMode",
                value: "1",
            })?;
            query_builder.append(QueryParam {
                key: "clientHdrCapVersion",
                value: "0",
            })?;
            query_builder.append(QueryParam {
                key: "clientHdrCapSupportedFlagsInUint32",
                value: "0",
            })?;
            query_builder.append(QueryParam {
                key: "clientHdrCapMetaDataId",
                value: "NV_STATIC_METADATA_TYPE_1",
            })?;
            query_builder.append(QueryParam {
                key: "clientHdrCapDisplayData",
                value: "0x0x0x0x0x0x0x0x0x0x0",
            })?;
        }

        query_builder.append(QueryParam {
            key: "localAudioPlayMode",
            value: if self.local_audio_play_mode { "1" } else { "0" },
        })?;

        let mut surround_audio_info = [0u8; 11];
        let surround_audio_info_value = u32_to_str(
            self.surround_audio_info.to_surround_audio_info(),
            &mut surround_audio_info,
        );
        query_builder.append(QueryParam {
            key: "surroundAudioInfo",
            value: surround_audio_info_value,
        })?;

        let mut gamepad_attached_mask_buffer = [0u8; 11];
        let gamepad_attached_mask_value = i32_to_str(
            self.gamepads_attached_mask,
            &mut gamepad_attached_mask_buffer,
        );
        query_builder.append(QueryParam {
            key: "remoteControllersBitmap",
            value: gamepad_attached_mask_value,
        })?;
        query_builder.append(QueryParam {
            key: "gcmap",
            value: gamepad_attached_mask_value,
        })?;

        query_builder.append(QueryParam {
            key: "gcpersist",
            value: if self.gamepads_persist_after_disconnect {
                "1"
            } else {
                "0"
            },
        })?;

        if let Some(web_host) = self.web_host_id {
            let mut web_host_buffer = [0u8; _];
            let web_host_value = u32_to_str(web_host, &mut web_host_buffer);

            query_builder.append(QueryParam {
                key: "hostid",
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

        let hdr = query_map.get("hdrMode").unwrap_or("0".into()) != "0";

        let surround_audio_info_raw = query_map
            .get("surroundAudioInfo")
            .ok()
            .map(|x| x.parse())
            .transpose()?
            .unwrap_or(AudioConfig::STEREO.to_surround_audio_info());
        let surround_audio_info = AudioConfig::from_surround_audio_info(surround_audio_info_raw);

        let local_audio_play_mode =
            query_map.get("localAudioPlayMode").unwrap_or("1".into()) != "0";

        // TODO: what to trust?
        let _gamepads_attached_mask: u32 = query_map
            .get("remoteControllersBitmap")
            .unwrap_or("0".into())
            .parse()?;
        let gamepads_attached_mask = query_map.get("gcmap").unwrap_or("0".into()).parse()?;

        let gamepads_persist_after_disconnect =
            query_map.get("gcpersist").unwrap_or("0".into()) != "0";

        let web_host = query_map
            .get("hostid")
            .ok()
            .map(|x| x.parse::<u32>())
            .transpose()?;

        Ok(Self {
            app_id,
            mode_width,
            mode_height,
            mode_fps,
            hdr,
            surround_audio_info,
            local_audio_play_mode,
            gamepads_attached_mask,
            gamepads_persist_after_disconnect,
            web_host_id: web_host,
        })
    }
}
