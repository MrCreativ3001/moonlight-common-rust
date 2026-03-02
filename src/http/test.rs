use std::{fmt::Debug, net::Ipv4Addr, str::FromStr};

use roxmltree::{Document, Node};
use tracing::debug;
use uuid::Uuid;

use crate::{
    PairStatus, ServerState, ServerVersion,
    http::{
        ClientInfo, DEFAULT_UNIQUE_ID, ParseError, QueryBuilder, QueryBuilderError, QueryParam,
        Request, TextResponse,
        app_list::{App, AppListRequest, AppListResponse},
        box_art::AppBoxArtRequest,
        cancel::{CancelRequest, CancelResponse},
        helper::fmt_write_to_buffer,
        launch::{ClientStreamRequest, LaunchResponse},
        resume::ResumeResponse,
        server_info::{ApolloPermissions, ServerInfoRequest, ServerInfoResponse},
    },
    init_test,
    mac::MacAddress,
    stream::{AesIv, AesKey, control::ActiveGamepads, video::ServerCodecModeSupport},
    test::init_test,
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

fn normalize_xml(doc: &str) -> String {
    doc.to_string()
        .replace("\n", "")
        .replace("\r", "")
        .replace("\t", "")
}
fn test_response<R>(response_expected: R, doc_expected: &str)
where
    R: TextResponse + Debug + PartialEq,
    R::Err: Debug,
{
    // stip some chars from the doc_expected
    let doc_expected = normalize_xml(doc_expected);
    debug!("Doc Expected: {doc_expected:?}");

    // test serialize
    let mut buffer = vec![0u8; 4096];
    let str = fmt_write_to_buffer(&mut buffer, |f| {
        response_expected.serialize_into(f).unwrap()
    });
    let doc = Document::parse(str).unwrap();

    let doc_expected_parsed = Document::parse(&doc_expected).unwrap();

    debug!("-- Serialization\nExpected: {doc_expected_parsed:?}\nGot: {doc:?}");

    assert!(nodes_equal(doc.root(), doc_expected_parsed.root()));

    // test deserialize
    let response = R::from_str(&doc_expected).unwrap();
    assert_eq!(response, response_expected);
}

fn nodes_equal(a: Node, b: Node) -> bool {
    a.tag_name() == b.tag_name()
        && a.attributes().eq(b.attributes())
        && a.children()
            .zip(b.children())
            .all(|(x, y)| nodes_equal(x, y))
        && a.text().map(str::trim) == b.text().map(str::trim)
}

#[test]
fn request_client_info() {
    init_test!();

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
    init_test();

    test_request(ServerInfoRequest {}, &[]);
}

#[test]
fn response_host_info_sunshine() {
    init_test();

    test_response(
        ServerInfoResponse {
            host_name: "PCNAME".to_string(),
            app_version: ServerVersion::new(7, 1, 431, -1),
            gfe_version: "3.23.0.74".to_string(),
            unique_id: Uuid::from_str("45924F0E-6465-B60F-B517-95DC63714036").unwrap(),
            https_port: 47984,
            external_port: 47989,
            max_luma_pixels_hevc: 0,
            mac: Some(MacAddress::from_str("00:B0:D0:63:C2:26").unwrap()),
            apollo_permissions: None,
            local_ip: Ipv4Addr::new(192, 168, 178, 140),
            server_codec_mode_support: ServerCodecModeSupport::from_bits(262145).unwrap(),
            pair_status: PairStatus::NotPaired,
            current_game: 0,
            apollo_game_uuid: None,
            state: ServerState::Free,
        },
        r#"
<?xml version="1.0" encoding="utf-8"?>
<root status_code="200">
<hostname>PCNAME</hostname>
<appversion>7.1.431.-1</appversion>
<GfeVersion>3.23.0.74</GfeVersion>
<uniqueid>45924F0E-6465-B60F-B517-95DC63714036</uniqueid>
<HttpsPort>47984</HttpsPort>
<ExternalPort>47989</ExternalPort>
<MaxLumaPixelsHEVC>0</MaxLumaPixelsHEVC>
<mac>00:B0:D0:63:C2:26</mac>
<LocalIP>192.168.178.140</LocalIP>
<ServerCodecModeSupport>262145</ServerCodecModeSupport>
<PairStatus>0</PairStatus>
<currentgame>0</currentgame>
<state>SUNSHINE_SERVER_FREE</state>
</root>
"#,
    );
}

#[test]
fn response_host_info_apollo() {
    init_test();

    test_response(
        ServerInfoResponse {
            host_name: "PCNAME".to_string(),
            app_version: ServerVersion::new(7, 1, 431, -1),
            gfe_version: "3.23.0.74".to_string(),
            unique_id: Uuid::from_str("C6D65CEB-F7EB-8F07-B501-D50ADBAC9117").unwrap(),
            https_port: 47984,
            external_port: 47989,
            max_luma_pixels_hevc: 1869449984,
            mac: Some(MacAddress::from_str("00:B0:D0:63:C2:26").unwrap()),
            apollo_permissions: Some(ApolloPermissions::LIST),
            local_ip: Ipv4Addr::new(127, 0, 0, 1),
            server_codec_mode_support: ServerCodecModeSupport::from_bits(769).unwrap(),
            pair_status: PairStatus::NotPaired,
            current_game: 0,
            apollo_game_uuid: Some(None),
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
fn response_host_info_auth_fail() {
    init_test();

    let text = normalize_xml(
        r#"
<?xml version="1.0" encoding="utf-8"?>
<root status_code="401" query="" status_message="The client is not authorized. Certificate verification failed."/>
    "#,
    );

    assert_eq!(
        ServerInfoResponse::from_str(&text).unwrap_err(),
        ParseError::InvalidXmlStatusCode {
            message: Some(
                "The client is not authorized. Certificate verification failed.".to_string()
            )
        }
    );
}

#[test]
fn request_app_list() {
    init_test();

    test_request(AppListRequest {}, &[]);
}

#[test]
fn response_app_list() {
    init_test();

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
                    title: "Steam Big Picture".to_string(),
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
</root>"#,
    );
}

#[test]
fn request_box_art() {
    init_test();

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
    init_test();

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
            ri_key_id: AesIv(0),
            ri_key: AesKey([0; _]),
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
    init_test();

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
fn response_launch_fail() {
    init_test();

    // See https://github.com/MrCreativ3001/moonlight-web-stream/issues/82
    let response = normalize_xml(
        r#"
<?xml version="1.0" encoding="utf-8"?>
<root status_code="-1" status_message="Failed to start the specified application">
<gamesession>0</gamesession>
</root>
"#,
    );

    assert_eq!(
        LaunchResponse::from_str(&response).unwrap_err(),
        ParseError::InvalidXmlStatusCode {
            message: Some("Failed to start the specified application".to_string())
        }
    );
}

#[test]
fn response_resume() {
    init_test();

    test_response(
        ResumeResponse {
            resume: 10,
            rtsp_session_url: "rtspenc://192.167.178.140:48010".to_string(),
        },
        r#"
<?xml version="1.0" encoding="utf-8"?>
<root status_code="200">
<resume>10</resume>
<sessionUrl0>rtspenc://192.167.178.140:48010</sessionUrl0>
</root>
        "#,
    );
}

#[test]
fn request_cancel() {
    init_test();

    test_request(CancelRequest {}, &[]);
}

#[test]
fn response_cancel() {
    init_test();

    test_response(
        CancelResponse { cancelled: false },
        r#"
<?xml version="1.0" encoding="utf-8"?>
<root status_code="200">
<cancel>0</cancel>
</root>
    "#,
    );

    test_response(
        CancelResponse { cancelled: true },
        r#"
<?xml version="1.0" encoding="utf-8"?>
<root status_code="200">
<cancel>1</cancel>
</root>
    "#,
    );
}
