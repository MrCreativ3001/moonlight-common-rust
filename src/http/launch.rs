use std::{
    fmt::{self, Write as _},
    str::FromStr,
};

use roxmltree::Document;

use crate::{
    http::{
        Endpoint, ParseError, QueryBuilder, QueryBuilderError, QueryIter, QueryParam, Request,
        TextResponse,
        helper::{
            fmt_write_to_buffer, i32_to_str, parse_xml_child_text, parse_xml_root_node, u32_to_str,
        },
    },
    stream::{AesIv, AesKey},
};

/// Launches a new session.
///
/// When there's already an active game this will fail to start a new session.
/// Then you should use [super::resume::ResumeEndpoint].
pub struct LaunchEndpoint;

impl Endpoint for LaunchEndpoint {
    type Request = ClientStreamRequest;
    type Response = LaunchResponse;

    fn path() -> &'static str {
        "/launch"
    }

    fn https_required() -> bool {
        true
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClientStreamRequest {
    pub app_id: u32,
    pub mode_width: u32,
    pub mode_height: u32,
    pub mode_fps: u32,
    pub sops: bool,
    pub hdr: bool,
    pub local_audio_play_mode: bool,
    pub gamepads_attached_mask: i32,
    pub gamepads_persist_after_disconnect: bool,
    pub ri_key: AesKey,
    pub ri_key_id: AesIv,
    pub additional_query_parameters: String,
}

impl Request for ClientStreamRequest {
    fn append_query_params(
        &self,
        query_builder: &mut impl QueryBuilder,
    ) -> Result<(), QueryBuilderError> {
        let launch_params = form_urlencoded::parse(self.additional_query_parameters.as_bytes());
        for (name, value) in launch_params {
            query_builder.append(QueryParam {
                key: &name,
                value: &value,
            })?;
        }

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
        query_builder.append(QueryParam {
            key: "sops",
            value: if self.sops { "1" } else { "0" },
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

        let mut ri_key_str_bytes = [0u8; 16 * 2];
        hex::encode_to_slice(&*self.ri_key, &mut ri_key_str_bytes).expect("encode ri key");
        query_builder.append(QueryParam {
            key: "rikey",
            value: str::from_utf8(&ri_key_str_bytes).expect("valid ri key str"),
        })?;

        let mut ri_key_id_str_bytes = [0; 11];
        let ri_key_id_str = u32_to_str(*self.ri_key_id, &mut ri_key_id_str_bytes);
        query_builder.append(QueryParam {
            key: "rikeyid",
            value: ri_key_id_str,
        })?;

        query_builder.append(QueryParam {
            key: "localAudioPlayMode",
            value: if self.local_audio_play_mode { "1" } else { "0" },
        })?;

        // TODO: what is this?
        // query_params.append(query_param("surroundAudioInfo", "todo"));

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

        Ok(())
    }

    fn from_query_params<'a, Q>(_query_iter: &mut Q) -> Result<Self, ()>
    where
        Q: QueryIter<'a>,
    {
        todo!()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct LaunchResponse {
    // TODO: what exactly is game_session used for?
    pub game_session: u32,
    pub rtsp_session_url: String,
}

impl TextResponse for LaunchResponse {
    fn serialize_into(&self, body_writer: &mut impl fmt::Write) -> fmt::Result {
        // XML header + root
        body_writer.write_str(r#"<?xml version="1.0" encoding="utf-8"?>"#)?;
        body_writer.write_str(r#"<root status_code="200">"#)?;

        // <gamesession>
        write!(
            body_writer,
            "<gamesession>{}</gamesession>",
            self.game_session
        )?;

        // <sessionUrl0>
        write!(
            body_writer,
            "<sessionUrl0>{}</sessionUrl0>",
            self.rtsp_session_url
        )?;

        // close root
        body_writer.write_str("</root>")?;

        Ok(())
    }
}

impl FromStr for LaunchResponse {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let doc = Document::parse(s)?;
        let root = parse_xml_root_node(&doc)?;

        Ok(LaunchResponse {
            game_session: parse_xml_child_text(root, "gamesession")?.parse()?,
            rtsp_session_url: parse_xml_child_text(root, "sessionUrl0")?.to_string(),
        })
    }
}
