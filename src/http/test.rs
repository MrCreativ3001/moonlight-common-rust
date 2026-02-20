use std::{fmt::Debug, net::Ipv4Addr, str::FromStr};

use roxmltree::Document;
use uuid::Uuid;

use crate::{
    PairStatus, ServerState, ServerVersion,
    http::{
        QueryBuilder, QueryBuilderError, QueryParam, Request, TextResponse,
        app_list::{App, AppListRequest, AppListResponse},
        box_art::AppBoxArtRequest,
        helper::fmt_write_to_buffer,
        host_info::{ApolloPermissions, HostInfoRequest, HostInfoResponse},
    },
    mac::MacAddress,
    stream::video::ServerCodecModeSupport,
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
    let response = R::from_str(&doc_expected).unwrap();
    assert_eq!(response, response_expected);
}

#[test]
fn request_host_info() {
    test_request(HostInfoRequest {}, &[]);
}

#[test]
fn response_host_info_sunshine() {
    test_response(
        HostInfoResponse {
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
        HostInfoResponse {
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
fn request_launch() {
    todo!()
}

#[test]
fn response_launch() {
    todo!()
}

#[test]
fn request_resume() {
    todo!()
}

#[test]
fn response_resume() {
    todo!()
}

#[test]
fn request_cancel() {
    todo!()
}

#[test]
fn response_cancel() {
    todo!()
}

#[test]
fn request_pair() {
    todo!()
}

#[test]
fn response_pair() {
    todo!()
}
