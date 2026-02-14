use std::{
    borrow::Cow, fmt, fmt::Write as _, num::ParseIntError, str::FromStr, string::FromUtf8Error,
};

use bitflags::bitflags;
use log::warn;
use roxmltree::{Document, Error, Node};
use thiserror::Error;
use uuid::{Uuid, adapter::Hyphenated};

use crate::{
    PairStatus, ParseServerStateError, ParseServerVersionError, ServerState, ServerType,
    ServerVersion,
    mac::{MacAddress, ParseMacAddressError},
    network::request_client::{LocalQueryParams, QueryBuilder, RequestClient, query_param},
    stream::video::ServerCodecModeSupport,
};

#[derive(Debug, Error)]
pub enum ApiError<RequestError> {
    #[error("{0}")]
    RequestClient(RequestError),
    #[error("the response is invalid xml")]
    ParseXmlError(#[from] Error),
    #[error("the returned xml doc has a non 200 status code")]
    InvalidXmlStatusCode { message: Option<String> },
    #[error("the returned xml doc doesn't have the root node")]
    XmlRootNotFound,
    #[error("the text contents of an xml node aren't present: {0}")]
    XmlTextNotFound(&'static str),
    #[error("detail was not found: {0}")]
    DetailNotFound(&'static str),
    #[error("{0}")]
    ParseServerStateError(#[from] ParseServerStateError),
    #[error("{0}")]
    ParseServerVersionError(#[from] ParseServerVersionError),
    #[error("parsing server codec mode support")]
    ParseServerCodecModeSupport,
    #[error("failed to parse the mac address")]
    ParseMacError(#[from] ParseMacAddressError),
    #[error("{0}")]
    ParseIntError(#[from] ParseIntError),
    #[error("{0}")]
    ParseUuidError(#[from] uuid::Error),
    #[error("{0}")]
    ParseHexError(#[from] hex::FromHexError),
    #[error("{0}")]
    Utf8Error(#[from] FromUtf8Error),
}

#[cfg(feature = "stream_proto")]
pub mod launch;
pub mod pair;
pub mod request_client;

pub mod backend;

// TODO: don't make this async depedendant but make this work in async and sync!

pub const DEFAULT_UNIQUE_ID: &str = "0123456789ABCDEF";

#[derive(Debug, Clone, Copy)]
pub struct ClientInfo<'a> {
    /// It's recommended to use the same (default) UID for all Moonlight clients so we can quit games started by other Moonlight clients.
    pub unique_id: &'a str,
    pub uuid: Uuid,
}

impl Default for ClientInfo<'static> {
    fn default() -> Self {
        Self {
            unique_id: DEFAULT_UNIQUE_ID,
            uuid: Uuid::new_v4(),
        }
    }
}

impl<'a> ClientInfo<'a> {
    // Requires 2 query params
    fn add_query_params(
        &self,
        uuid_bytes: &'a mut [u8; Hyphenated::LENGTH],
        query_params: &mut impl QueryBuilder<'a>,
    ) {
        query_params.push((Cow::Borrowed("uniqueid"), Cow::Borrowed(self.unique_id)));

        self.uuid.to_hyphenated_ref().encode_lower(uuid_bytes);
        let uuid_str = str::from_utf8(uuid_bytes).expect("uuid string");

        query_params.push((Cow::Borrowed("uuid"), Cow::Borrowed(uuid_str)));
    }
}

fn xml_child_text<'doc, 'node, C: RequestClient>(
    list_node: Node<'node, 'doc>,
    name: &'static str,
) -> Result<&'node str, ApiError<C::Error>>
where
    'node: 'doc,
{
    let node = list_node
        .children()
        .find(|node| node.tag_name().name() == name)
        .ok_or(ApiError::<C::Error>::DetailNotFound(name))?;
    let content = node
        .text()
        .ok_or(ApiError::<C::Error>::XmlTextNotFound(name))?;

    Ok(content)
}

fn xml_root_node<'doc, C>(doc: &'doc Document) -> Result<Node<'doc, 'doc>, ApiError<C>> {
    let root = doc
        .root()
        .children()
        .find(|node| node.tag_name().name() == "root")
        .ok_or(ApiError::XmlRootNotFound)?;

    let status_code = root
        .attribute("status_code")
        .ok_or(ApiError::DetailNotFound("status_code"))?
        .parse::<u32>()?;

    if status_code / 100 == 4 {
        return Err(ApiError::InvalidXmlStatusCode {
            message: root.attribute("status_message").map(str::to_string),
        });
    }

    Ok(root)
}

// TODO: maybe move this into the stream/bindings.rs?
// Apollo Permissions
bitflags! {
    /// The permissions of a client: https://github.com/ClassicOldSong/Apollo/blob/a40b179886856bba1dfe311f430a25b9f3c44390/src/crypto.h#L42-L74
    #[derive(Debug, Clone)]
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

#[derive(Debug, Clone)]
pub struct HostInfo {
    pub host_name: String,
    pub app_version: ServerVersion,
    pub gfe_version: String,
    pub unique_id: Uuid,
    pub https_port: u16,
    pub external_port: u16,
    pub max_luma_pixels_hevc: u32,
    pub mac: Option<MacAddress>,
    pub local_ip: String,
    pub server_codec_mode_support: ServerCodecModeSupport,
    pub pair_status: PairStatus,
    pub current_game: u32,
    pub state_string: String,
    pub state: ServerState,
    /// Apollo Extension
    pub apollo_permissions: Option<ApolloPermissions>,
}

pub async fn host_info<C: RequestClient>(
    client: &mut C,
    use_https: bool,
    hostport: &str,
    info: Option<ClientInfo<'_>>,
) -> Result<HostInfo, ApiError<C::Error>> {
    let mut query_params = LocalQueryParams::<2>::default();

    let mut uuid_bytes = [0; _];
    if let Some(info) = info {
        info.add_query_params(&mut uuid_bytes, &mut query_params);
    }

    let response = if use_https {
        client
            .send_https_request_text_response(hostport, "serverinfo", &query_params)
            .await
            .map_err(ApiError::RequestClient)?
    } else {
        client
            .send_http_request_text_response(hostport, "serverinfo", &query_params)
            .await
            .map_err(ApiError::RequestClient)?
    };

    let doc = Document::parse(response.as_ref())?;
    let root = xml_root_node(&doc)?;

    let state_string = xml_child_text::<C>(root, "state")?.to_string();

    let mac = match xml_child_text::<C>(root, "mac") {
        Ok(mac) => match mac.parse()? {
            mac if mac == MacAddress::from_bytes([0u8; 6]) => None,
            mac => Some(mac),
        },
        Err(_) => {
            warn!("failed to get mac from host response");
            None
        }
    };

    let mut app_version: ServerVersion = xml_child_text::<C>(root, "appversion")?.parse()?;

    // https://github.com/ClassicOldSong/Apollo/blob/a40b179886856bba1dfe311f430a25b9f3c44390/src/nvhttp.cpp#L931
    let apollo_permissions = match xml_child_text::<C>(root, "Permission") {
        Ok(permissions) => Some(ApolloPermissions::from_bits_truncate(permissions.parse()?)),
        Err(_) => None,
    };
    if apollo_permissions.is_some() {
        app_version.server_type = ServerType::Apollo;
    }

    Ok(HostInfo {
        host_name: xml_child_text::<C>(root, "hostname")?.to_string(),
        app_version,
        gfe_version: xml_child_text::<C>(root, "GfeVersion")?.to_string(),
        unique_id: xml_child_text::<C>(root, "uniqueid")?.parse()?,
        https_port: xml_child_text::<C>(root, "HttpsPort")?.parse()?,
        external_port: xml_child_text::<C>(root, "ExternalPort")?.parse()?,
        max_luma_pixels_hevc: xml_child_text::<C>(root, "MaxLumaPixelsHEVC")?.parse()?,
        mac,
        local_ip: xml_child_text::<C>(root, "LocalIP")?.to_string(),
        server_codec_mode_support: ServerCodecModeSupport::from_bits_retain(
            xml_child_text::<C>(root, "ServerCodecModeSupport")?.parse()?,
        ),
        pair_status: if xml_child_text::<C>(root, "PairStatus")?.parse::<u32>()? == 0 {
            PairStatus::NotPaired
        } else {
            PairStatus::Paired
        },
        current_game: xml_child_text::<C>(root, "currentgame")?.parse()?,
        state: ServerState::from_str(&state_string)?,
        state_string,
        apollo_permissions,
    })
}

// Pairing: https://github.com/moonlight-stream/moonlight-android/blob/master/app/src/main/java/com/limelight/nvstream/http/PairingManager.java#L185

fn xml_child_paired<'doc, 'node, C: RequestClient>(
    list_node: Node<'node, 'doc>,
    name: &'static str,
) -> Result<PairStatus, ApiError<C::Error>>
where
    'node: 'doc,
{
    let content = xml_child_text::<C>(list_node, name)?.parse::<i32>()?;

    Ok(if content == 1 {
        PairStatus::Paired
    } else {
        PairStatus::NotPaired
    })
}

#[derive(Debug, Clone)]
pub struct App {
    pub id: u32,
    pub title: String,
    pub is_hdr_supported: bool,
}

#[derive(Debug, Clone)]
pub struct ServerAppListResponse {
    pub apps: Vec<App>,
}

pub async fn host_app_list<C: RequestClient>(
    client: &mut C,
    https_hostport: &str,
    info: ClientInfo<'_>,
) -> Result<ServerAppListResponse, ApiError<C::Error>> {
    let mut query_params = LocalQueryParams::<2>::default();

    let mut uuid_bytes = [0; _];
    info.add_query_params(&mut uuid_bytes, &mut query_params);

    let response = client
        .send_https_request_text_response(https_hostport, "applist", &query_params)
        .await
        .map_err(ApiError::RequestClient)?;

    let doc = Document::parse(response.as_ref())?;
    let root = xml_root_node(&doc)?;

    let apps = root
        .children()
        .filter(|node| node.tag_name().name() == "App")
        .map(|app_node| {
            let title = xml_child_text::<C>(app_node, "AppTitle")?.to_string();

            let id = xml_child_text::<C>(app_node, "ID")?.parse()?;

            let is_hdr_supported = xml_child_text::<C>(app_node, "IsHdrSupported")
                .unwrap_or("0")
                .parse::<u32>()?
                == 1;

            Ok(App {
                id,
                title,
                is_hdr_supported,
            })
        })
        .collect::<Result<Vec<_>, ApiError<_>>>()?;

    Ok(ServerAppListResponse { apps })
}

#[derive(Debug, Clone)]
pub struct ClientAppBoxArtRequest {
    pub app_id: u32,
}

pub async fn host_app_box_art<C: RequestClient>(
    client: &mut C,
    https_address: &str,
    info: ClientInfo<'_>,
    request: ClientAppBoxArtRequest,
) -> Result<C::Bytes, ApiError<C::Error>> {
    // Assets: https://github.com/moonlight-stream/moonlight-android/blob/master/app/src/main/java/com/limelight/nvstream/http/NvHTTP.java#L721
    let mut query_params = LocalQueryParams::<{ 2 + 3 }>::default();

    let mut uuid_bytes = [0; _];
    info.add_query_params(&mut uuid_bytes, &mut query_params);

    let mut appid_buffer = [0u8; _];
    let appid = u32_to_str(request.app_id, &mut appid_buffer);
    query_params.push(query_param("appid", appid));

    query_params.push(query_param("AssetType", "2"));
    query_params.push(query_param("AssetIdx", "0"));

    let response = client
        .send_https_request_data_response(https_address, "appasset", &query_params)
        .await
        .map_err(ApiError::RequestClient)?;

    Ok(response)
}

pub async fn host_cancel<C: RequestClient>(
    client: &mut C,
    https_hostport: &str,
    info: ClientInfo<'_>,
) -> Result<bool, ApiError<C::Error>> {
    let mut query_params: LocalQueryParams<'_, 2> = LocalQueryParams::default();

    let mut uuid_bytes = [0; _];
    info.add_query_params(&mut uuid_bytes, &mut query_params);

    let response = client
        .send_https_request_text_response(https_hostport, "cancel", &query_params)
        .await
        .map_err(ApiError::RequestClient)?;

    let doc = Document::parse(response.as_ref())?;
    let root = doc
        .root()
        .children()
        .find(|node| node.tag_name().name() == "root")
        .ok_or(ApiError::XmlRootNotFound)?;

    let cancel = xml_child_text::<C>(root, "cancel")?.trim();

    Ok(cancel != "0")
}

struct CounterWriter<'a> {
    buf: &'a mut [u8],
    pos: usize, // tracks how many bytes have been written
}

impl<'a> fmt::Write for CounterWriter<'a> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let bytes = s.as_bytes();
        if self.pos + bytes.len() > self.buf.len() {
            return Err(fmt::Error); // buffer overflow
        }
        self.buf[self.pos..self.pos + bytes.len()].copy_from_slice(bytes);
        self.pos += bytes.len();
        Ok(())
    }
}

fn u32_to_str(num: u32, buffer: &mut [u8; 11]) -> &str {
    fmt_write_to_buffer(buffer, |writer| write!(writer, "{num}").expect("write u32"))
}
fn fmt_write_to_buffer(buffer: &mut [u8], fmt: impl FnOnce(&mut CounterWriter)) -> &str {
    let mut writer = CounterWriter {
        buf: buffer,
        pos: 0,
    };

    fmt(&mut writer);

    let pos = writer.pos;

    str::from_utf8(&buffer[0..pos]).expect("valid utf8 bytes")
}
