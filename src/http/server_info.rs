use std::{fmt, fmt::Write as _, net::Ipv4Addr, str::FromStr};

use bitflags::bitflags;
use roxmltree::Document;
use tracing::warn;
use uuid::Uuid;

use crate::{
    PairStatus, ServerState, ServerType, ServerVersion,
    http::{
        Endpoint, ParseError, QueryBuilder, QueryBuilderError, QueryIter, Request, TextResponse,
        helper::{
            fmt_write_to_buffer, parse_xml_child_text, parse_xml_root_node, serialize_text_xml,
        },
    },
    mac::MacAddress,
    stream::video::ServerCodecModeSupport,
};

/// Queries information about the host.
///
/// Some information might be hidden if the client is not authenticated over https.
/// This might include:
/// - mac address
pub struct ServerInfoEndpoint;

impl Endpoint for ServerInfoEndpoint {
    type Request = ServerInfoRequest;
    type Response = ServerInfoResponse;

    fn path() -> &'static str {
        "/serverinfo"
    }

    fn https_required() -> bool {
        false
    }
}

#[derive(Debug, PartialEq)]
pub struct ServerInfoRequest {}

impl Request for ServerInfoRequest {
    fn append_query_params(
        &self,
        _query_builder: &mut impl QueryBuilder,
    ) -> Result<(), QueryBuilderError> {
        Ok(())
    }
    fn from_query_params<'a, Q>(_query_iter: &mut Q) -> Result<Self, ()>
    where
        Q: QueryIter<'a>,
    {
        Ok(Self {})
    }
}

// TODO: maybe move this into the stream/bindings.rs?
// Apollo Permissions
bitflags! {
    /// The permissions of a client: https://github.com/ClassicOldSong/Apollo/blob/a40b179886856bba1dfe311f430a25b9f3c44390/src/crypto.h#L42-L74
    #[derive(Debug, Clone, PartialEq)]
    pub struct ApolloPermissions: u32 {
        const _RESERVED = 0b00000001;

        const _INPUT           = Self::_RESERVED.bits() << 8;
        #[allow(clippy::identity_op)]
        const INPUT_CONTROLLER = Self::_INPUT.bits() << 0;
        const INPUT_TOUCH      = Self::_INPUT.bits() << 1;
        const INPUT_PEN        = Self::_INPUT.bits() << 2;
        const INPUT_MOUSE      = Self::_INPUT.bits() << 3;
        const INPUT_KEYBOARD        = Self::_INPUT.bits() << 4;
        const _ALL_INPUTS      = Self::INPUT_CONTROLLER.bits() | Self::INPUT_TOUCH.bits() | Self::INPUT_PEN.bits() | Self::INPUT_MOUSE.bits() | Self::INPUT_KEYBOARD.bits();

        const _OPERATION       = Self::_INPUT.bits() << 8;
        #[allow(clippy::identity_op)]
        const CLIPBOARD_SET    = Self::_OPERATION.bits() << 0;
        const CLIPBOARD_READ   = Self::_OPERATION.bits() << 1;
        const FILE_UPLOAD      = Self::_OPERATION.bits() << 2;
        const FILE_DOWNLOAD    = Self::_OPERATION.bits() << 3;
        const SERVER_COMMAND   = Self::_OPERATION.bits() << 4;
        const _ALL_OPERATIONS  = Self::CLIPBOARD_SET.bits() | Self::CLIPBOARD_READ.bits() | Self::FILE_UPLOAD.bits() | Self::FILE_DOWNLOAD.bits() | Self::SERVER_COMMAND.bits();

        const _ACTION          = Self::_OPERATION.bits() << 8;
        #[allow(clippy::identity_op)]
        const LIST             = Self::_ACTION.bits() << 0;
        const VIEW             = Self::_ACTION.bits() << 1;
        const LAUNCH           = Self::_ACTION.bits() << 2;
        const _ALLOW_VIEW      = Self::VIEW.bits() | Self::LAUNCH.bits();
        const _ALL_ACTIONS     = Self::LIST.bits() | Self::VIEW.bits() | Self::LAUNCH.bits();

        const _DEFAULT         = Self::VIEW.bits() | Self::LIST.bits();
        const _NO              = 0;
        const _ALL             = Self::_ALL_INPUTS.bits() | Self::_ALL_OPERATIONS.bits() | Self::_ALL_ACTIONS.bits();
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ServerInfoResponse {
    pub host_name: String,
    pub app_version: ServerVersion,
    pub gfe_version: String,
    pub unique_id: Uuid,
    pub https_port: u16,
    pub external_port: u16,
    pub max_luma_pixels_hevc: u32,
    pub mac: Option<MacAddress>,
    pub local_ip: Ipv4Addr,
    pub server_codec_mode_support: ServerCodecModeSupport,
    pub pair_status: PairStatus,
    pub current_game: u32,
    pub state: ServerState,
    /// Apollo Extension
    pub apollo_permissions: Option<ApolloPermissions>,
}

impl TextResponse for ServerInfoResponse {
    fn serialize_into(&self, body_writer: &mut impl fmt::Write) -> fmt::Result {
        // XML header + root
        body_writer.write_str(r#"<?xml version="1.0" encoding="utf-8"?>"#)?;
        body_writer.write_str(r#"<root status_code="200">"#)?;

        // <hostname>
        body_writer.write_str("<hostname>")?;
        serialize_text_xml(body_writer, &self.host_name)?;
        body_writer.write_str("</hostname>")?;

        // <appversion>
        body_writer.write_str("<appversion>")?;
        write!(body_writer, "{}", self.app_version)?;
        body_writer.write_str("</appversion>")?;

        // <GfeVersion>
        body_writer.write_str("<GfeVersion>")?;
        serialize_text_xml(body_writer, &self.gfe_version)?;
        body_writer.write_str("</GfeVersion>")?;

        // <uniqueid>
        body_writer.write_str("<uniqueid>")?;
        write!(body_writer, "{}", self.unique_id)?;
        body_writer.write_str("</uniqueid>")?;

        // <HttpsPort>
        write!(body_writer, "<HttpsPort>{}</HttpsPort>", self.https_port)?;

        // <ExternalPort>
        write!(
            body_writer,
            "<ExternalPort>{}</ExternalPort>",
            self.external_port
        )?;

        // <MaxLumaPixelsHEVC>
        write!(
            body_writer,
            "<MaxLumaPixelsHEVC>{}</MaxLumaPixelsHEVC>",
            self.max_luma_pixels_hevc
        )?;

        // <mac>
        body_writer.write_str("<mac>")?;
        match &self.mac {
            Some(mac) => write!(body_writer, "{mac}")?,
            None => body_writer.write_str("00:00:00:00:00:00")?,
        }
        body_writer.write_str("</mac>")?;

        // <Permission> (Apollo extension)
        if let Some(permissions) = &self.apollo_permissions {
            write!(
                body_writer,
                "<Permission>{}</Permission>",
                permissions.bits()
            )?;
        }

        // <LocalIP>
        body_writer.write_str("<LocalIP>")?;
        // max ip characters, "255.255.255.255", len = 16
        let mut buffer = [0; 16];
        let ip = fmt_write_to_buffer(&mut buffer, |f| {
            write!(f, "{}", self.local_ip).expect("failed to write ip")
        });

        serialize_text_xml(body_writer, ip)?;
        body_writer.write_str("</LocalIP>")?;

        // <ServerCodecModeSupport>
        write!(
            body_writer,
            "<ServerCodecModeSupport>{}</ServerCodecModeSupport>",
            self.server_codec_mode_support.bits()
        )?;

        // <PairStatus>
        let pair_value = match self.pair_status {
            PairStatus::NotPaired => 0,
            PairStatus::Paired => 1,
        };
        write!(body_writer, "<PairStatus>{pair_value}</PairStatus>")?;

        // <currentgame>
        write!(
            body_writer,
            "<currentgame>{}</currentgame>",
            self.current_game
        )?;

        // <currentgameuuid/> (empty)
        body_writer.write_str("<currentgameuuid/>")?;

        // <state>
        body_writer.write_str("<state>")?;
        serialize_text_xml(body_writer, self.state.as_str())?;
        body_writer.write_str("</state>")?;

        // close root
        body_writer.write_str("</root>")?;

        Ok(())
    }
}

impl FromStr for ServerInfoResponse {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let doc = Document::parse(s.as_ref())?;
        let root = parse_xml_root_node(&doc)?;

        let state_string = parse_xml_child_text(root, "state")?.to_string();

        let mac = match parse_xml_child_text(root, "mac") {
            Ok(mac) => match mac.parse()? {
                mac if mac == MacAddress::from_bytes([0u8; 6]) => None,
                mac => Some(mac),
            },
            Err(_) => {
                warn!("failed to get mac from host response");
                None
            }
        };

        let mut app_version: ServerVersion = parse_xml_child_text(root, "appversion")?.parse()?;

        // https://github.com/ClassicOldSong/Apollo/blob/a40b179886856bba1dfe311f430a25b9f3c44390/src/nvhttp.cpp#L931
        let apollo_permissions = match parse_xml_child_text(root, "Permission") {
            Ok(permissions) => Some(ApolloPermissions::from_bits_truncate(permissions.parse()?)),
            Err(_) => None,
        };
        if apollo_permissions.is_some() {
            app_version.server_type = ServerType::Apollo;
        }

        // Real Nvidia host software (GeForce Experience and RTX Experience) both use the 'Mjolnir'
        // codename in the state field and no version of Sunshine does.
        if state_string.contains("Mjolnir") {
            app_version.server_type = ServerType::NvidiaGameStream;
        }

        Ok(ServerInfoResponse {
            host_name: parse_xml_child_text(root, "hostname")?.to_string(),
            app_version,
            gfe_version: parse_xml_child_text(root, "GfeVersion")?.to_string(),
            unique_id: parse_xml_child_text(root, "uniqueid")?.parse()?,
            https_port: parse_xml_child_text(root, "HttpsPort")?.parse()?,
            external_port: parse_xml_child_text(root, "ExternalPort")?.parse()?,
            max_luma_pixels_hevc: parse_xml_child_text(root, "MaxLumaPixelsHEVC")?.parse()?,
            mac,
            local_ip: parse_xml_child_text(root, "LocalIP")?.parse()?,
            server_codec_mode_support: ServerCodecModeSupport::from_bits_retain(
                parse_xml_child_text(root, "ServerCodecModeSupport")?.parse()?,
            ),
            pair_status: if parse_xml_child_text(root, "PairStatus")?.parse::<u32>()? == 0 {
                PairStatus::NotPaired
            } else {
                PairStatus::Paired
            },
            current_game: parse_xml_child_text(root, "currentgame")?.parse()?,
            state: ServerState::from_str(&state_string)?,
            apollo_permissions,
        })
    }
}
