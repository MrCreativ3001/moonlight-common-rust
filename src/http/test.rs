use std::{fmt::Debug, net::Ipv4Addr, str::FromStr};

use roxmltree::Document;
use uuid::Uuid;

use crate::{
    PairStatus, ServerState, ServerVersion,
    http::{
        ClientInfo, DEFAULT_UNIQUE_ID, QueryBuilder, QueryBuilderError, QueryParam, Request,
        TextResponse,
        app_list::{App, AppListRequest, AppListResponse},
        box_art::AppBoxArtRequest,
        cancel::{CancelRequest, CancelResponse},
        helper::fmt_write_to_buffer,
        launch::{ClientStreamRequest, LaunchResponse},
        server_info::{ApolloPermissions, ServerInfoRequest, ServerInfoResponse},
    },
    mac::MacAddress,
    stream::{control::ActiveGamepads, video::ServerCodecModeSupport},
};

#[derive(Debug, Default)]
struct TestQueryBuilder {
    params: Vec<(String, String)>,
}

impl QueryBuilder for TestQueryBuilder {
    fn append(&mut self, param: QueryParam) -> Result<(), QueryBuilderError> {
        self.params
            .push((param.key.to_string(), param.value.to_string()));
        Ok(())
    }
}

fn test_request<R>(request_expected: R, query_params_expected: &[QueryParam])
where
    R: Request + Debug + PartialEq,
{
    // test serialize
    let mut query_params = TestQueryBuilder::default();

    request_expected
        .append_query_params(&mut query_params)
        .unwrap();

    assert_eq!(query_params.params.len(), query_params_expected.len());
    for expected in query_params_expected {
        if query_params
            .params
            .iter()
            .find(|param| param.0 == expected.key && param.1 == expected.value)
            .is_none()
        {
            panic!(
                "Couldn't find query param: {expected:?}, Got: \n{:?}",
                query_params.params
            );
        };
    }

    // test deserialize
    let request = R::from_query_params(&mut query_params_expected.iter()).unwrap();
    assert_eq!(request, request_expected);
}

fn test_response<R>(response_expected: R, doc_expected: &str)
where
    R: TextResponse + Debug + PartialEq,
    R::Err: Debug,
{
    // test serialize
    let mut buffer = vec![0u8; 4096];
    let str = fmt_write_to_buffer(&mut buffer, |f| {
        response_expected.serialize_into(f).unwrap()
    });
    let doc = Document::parse(str).unwrap();
    assert_eq!(doc.root(), Document::parse(doc_expected).unwrap().root());

    // test deserialize
    let response = R::from_str(doc_expected).unwrap();
    assert_eq!(response, response_expected);
}

#[test]
fn request_client_info() {
    let uuid = Uuid::from_u128(4522875942567894520547);

    test_request(
        ClientInfo {
            unique_id: DEFAULT_UNIQUE_ID,
            uuid,
        },
        &[
            QueryParam {
                key: "uniqueid",
                value: DEFAULT_UNIQUE_ID,
            },
            QueryParam {
                key: "uuid",
                value: &uuid.to_hyphenated().to_string(),
            },
        ],
    );
}

#[test]
fn request_host_info() {
    test_request(ServerInfoRequest {}, &[]);
}

#[test]
fn response_host_info_sunshine() {
    test_response(
        ServerInfoResponse {
            host_name: "PCNAME".to_string(),
            app_version: ServerVersion::new(7, 1, 431, -1),
            gfe_version: "3.23.0.74".to_string(),
            unique_id: Uuid::from_str("C6D65CEB-F7EB-8F07-B501-D50ADBAC9117").unwrap(),
            https_port: 47989,
            external_port: 47989,
            max_luma_pixels_hevc: 1869449984,
            mac: Some(MacAddress::from_str("00:B0:D0:63:C2:26").unwrap()),
            apollo_permissions: None,
            local_ip: Ipv4Addr::new(127, 0, 0, 1),
            server_codec_mode_support: ServerCodecModeSupport::from_bits(769).unwrap(),
            pair_status: PairStatus::NotPaired,
            current_game: 0,
            state: ServerState::Free,
        },
        r#"
<?xml version="1.0" encoding="utf-8"?>
<root status_code="200">
	<hostname>PCNAME</hostname>
	<appversion>7.1.431.-1</appversion>
	<GfeVersion>3.23.0.74</GfeVersion>
	<uniqueid>C6D65CEB-F7EB-8F07-B501-D50ADBAC9117</uniqueid>
	<HttpsPort>47984</HttpsPort>
	<ExternalPort>47989</ExternalPort>
	<MaxLumaPixelsHEVC>1869449984</MaxLumaPixelsHEVC>
	<mac>00:B0:D0:63:C2:26</mac>
	<LocalIP>127.0.0.1</LocalIP>
	<ServerCodecModeSupport>769</ServerCodecModeSupport>
	<PairStatus>0</PairStatus>
	<currentgame>0</currentgame>
	<currentgameuuid/>
	<state>SUNSHINE_SERVER_FREE</state>
</root>
"#,
    );
}

#[test]
fn response_host_info_apollo() {
    test_response(
        ServerInfoResponse {
            host_name: "PCNAME".to_string(),
            app_version: ServerVersion::new(7, 1, 431, -1),
            gfe_version: "3.23.0.74".to_string(),
            unique_id: Uuid::from_str("C6D65CEB-F7EB-8F07-B501-D50ADBAC9117").unwrap(),
            https_port: 47989,
            external_port: 47989,
            max_luma_pixels_hevc: 1869449984,
            mac: Some(MacAddress::from_str("00:B0:D0:63:C2:26").unwrap()),
            apollo_permissions: Some(ApolloPermissions::LIST),
            local_ip: Ipv4Addr::new(127, 0, 0, 1),
            server_codec_mode_support: ServerCodecModeSupport::from_bits(769).unwrap(),
            pair_status: PairStatus::NotPaired,
            current_game: 0,
            state: ServerState::Free,
        },
        r#"
<?xml version="1.0" encoding="utf-8"?>
<root status_code="200">
	<hostname>PCNAME</hostname>
	<appversion>7.1.431.-1</appversion>
	<GfeVersion>3.23.0.74</GfeVersion>
	<uniqueid>C6D65CEB-F7EB-8F07-B501-D50ADBAC9117</uniqueid>
	<HttpsPort>47984</HttpsPort>
	<ExternalPort>47989</ExternalPort>
	<MaxLumaPixelsHEVC>1869449984</MaxLumaPixelsHEVC>
	<mac>00:B0:D0:63:C2:26</mac>
    <Permission>16777216</Permission>
	<LocalIP>127.0.0.1</LocalIP>
	<ServerCodecModeSupport>769</ServerCodecModeSupport>
	<PairStatus>0</PairStatus>
	<currentgame>0</currentgame>
	<currentgameuuid/>
	<state>SUNSHINE_SERVER_FREE</state>
</root>
"#,
    );
}

#[test]
fn request_app_list() {
    test_request(AppListRequest {}, &[]);
}

#[test]
fn response_app_list() {
    test_response(
        AppListResponse {
            apps: vec![
                App {
                    id: 881448767,
                    title: "Desktop".to_string(),
                    is_hdr_supported: false,
                },
                App {
                    id: 1093255277,
                    title: "Stream Big Picture".to_string(),
                    is_hdr_supported: true,
                },
            ],
        },
        r#"
<?xml version="1.0" encoding="utf-8"?>
<root status_code="200">
	<App>
		<IsHdrSupported>0</IsHdrSupported>
		<AppTitle>Desktop</AppTitle>
		<ID>881448767</ID>
	</App>
	<App>
		<IsHdrSupported>1</IsHdrSupported>
		<AppTitle>Steam Big Picture</AppTitle>
		<ID>1093255277</ID>
	</App>
</root>
        "#,
    );
}

#[test]
fn request_boxart() {
    test_request(
        AppBoxArtRequest { app_id: 1093255277 },
        &[
            QueryParam {
                key: "appid",
                value: "1093255277",
            },
            QueryParam {
                key: "AssetType",
                value: "2",
            },
            QueryParam {
                key: "AssetIdx",
                value: "0",
            },
        ],
    );
}

#[test]
fn request_launch_and_resume() {
    // TODO: use values
    test_request(
        ClientStreamRequest {
            app_id: 10,
            mode_width: 1920,
            mode_height: 1080,
            mode_fps: 60,
            hdr: false,
            local_audio_play_mode: false,
            gamepads_attached_mask: ActiveGamepads::GAMEPAD_1.bits() as i32,
            gamepads_persist_after_disconnect: false,
            sops: true,
            ri_key_id: 0,
            ri_key: [0; _],
            additional_query_parameters: "&corever=1".to_string(),
        },
        &[
            QueryParam {
                key: "appid",
                value: "10",
            },
            QueryParam {
                key: "rikey",
                value: "",
            },
            QueryParam {
                key: "rikeyid",
                value: "0",
            },
            QueryParam {
                key: "localAudioPlayMode",
                value: "0",
            },
            QueryParam {
                key: "surroundAudioInfo",
                value: "0",
            },
            QueryParam {
                key: "remoteControllerBitmap",
                value: "1",
            },
            QueryParam {
                key: "gcmap",
                value: "0",
            },
            QueryParam {
                key: "gcpersist",
                value: "0",
            },
        ],
    );
}

#[test]
fn response_launch() {
    test_response(
        LaunchResponse {
            game_session: 10,
            rtsp_session_url: "rtspenc://192.167.178.140:48010".to_string(),
        },
        r#"
<?xml version="1.0" encoding="utf-8"?>
<root status_code="200">
    <gamesession>10</gamesession>
    <sessionUrl0>rtspenc://192.167.178.140:48010</sessionUrl0>
</root>
        "#,
    );
}

#[test]
fn response_resume() {
    test_response(
        LaunchResponse {
            game_session: 10,
            rtsp_session_url: "rtspenc://192.167.178.140:48010".to_string(),
        },
        r#"
<?xml version="1.0" encoding="utf-8"?>
<root status_code="200">
    <gamesession>10</gamesession>
    <sessionUrl0>rtspenc://192.167.178.140:48010</sessionUrl0>
</root>
        "#,
    );
}

#[test]
fn request_cancel() {
    test_request(CancelRequest {}, &[]);
}

#[test]
fn response_cancel() {
    test_response(
        CancelResponse { cancel: false },
        r#"
<?xml version="1.0" encoding="utf-8"?>
<root status_code="200">
    <cancel>0</cancel>
</root>
    "#,
    );

    test_response(
        CancelResponse { cancel: true },
        r#"
<?xml version="1.0" encoding="utf-8"?>
<root status_code="200">
    <cancel>100</cancel>
</root>
    "#,
    );
}
