// TODO: test one full rtsp setup

use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6},
    str::FromStr,
};

use crate::stream::proto::rtsp::{
    RtspClient, RtspClientConfig, RtspInput, RtspOutput,
    raw::{
        RtspAddr, RtspCommand, RtspProtocol, RtspRequest, RtspRequestMessage, RtspResponse,
        RtspResponseMessage,
    },
};

#[test]
fn rtsp_command() {
    assert_eq!(format!("{}", RtspCommand::Options), "OPTIONS");
    assert_eq!(format!("{}", RtspCommand::Describe), "DESCRIBE");
    assert_eq!(format!("{}", RtspCommand::Setup), "SETUP");
    assert_eq!(format!("{}", RtspCommand::Announce), "ANNOUNCE");
    assert_eq!(format!("{}", RtspCommand::Play), "PLAY");
}

#[test]
fn rtsp_target() {
    assert_eq!(
        format!(
            "{}",
            RtspAddr {
                encrypted: false,
                addr: SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), 80).into(),
            }
        ),
        "rtsp://127.0.0.1:80"
    );
    assert_eq!(
        format!(
            "{}",
            RtspAddr {
                encrypted: true,
                addr: SocketAddrV4::new(Ipv4Addr::new(192, 168, 1, 100), 48010).into(),
            }
        ),
        "rtspenc://192.168.1.100:48010"
    );

    assert_eq!(
        format!(
            "{}",
            RtspAddr {
                encrypted: false,
                addr: SocketAddrV6::new(Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 1), 80, 0, 0).into(),
            }
        ),
        "rtsp://[::1]:80"
    );
    assert_eq!(
        format!(
            "{}",
            RtspAddr {
                encrypted: true,
                addr: SocketAddrV6::new(Ipv6Addr::new(0xfd00, 0, 0, 0, 0, 0, 0, 0x10), 48010, 0, 0)
                    .into(),
            }
        ),
        "rtspenc://[fd00::10]:48010"
    );
}

#[test]
fn rtsp_protocol() {
    let test = |protocol: RtspProtocol, serialized: &str| {
        assert_eq!(format!("{}", protocol), serialized);
        assert_eq!(RtspProtocol::from_str(serialized).unwrap(), protocol);
    };

    test(RtspProtocol::V1_0, "RTSP/1.0");
}

#[test]
fn rtsp_request_message() {
    assert_eq!(
        format!(
            "{}",
            RtspRequestMessage {
                command: RtspCommand::Options,
                target: RtspAddr {
                    encrypted: false,
                    addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 80),
                }
                .to_string(),
                protocol: RtspProtocol::V1_0,
            }
        ),
        "OPTIONS rtsp://127.0.0.1:80 RTSP/1.0"
    );
    assert_eq!(
        format!(
            "{}",
            RtspRequestMessage {
                command: RtspCommand::Setup,
                target: "streamid=audio".to_string(),
                protocol: RtspProtocol::V1_0,
            }
        ),
        "SETUP streamid=audio RTSP/1.0"
    );
}

#[test]
fn rtsp_request() {
    assert_eq!(
        format!(
            "{}",
            RtspRequest {
                message: RtspRequestMessage {
                    command: RtspCommand::Describe,
                    target: RtspAddr {
                        encrypted: false,
                        addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 80),
                    }
                    .to_string(),
                    protocol: RtspProtocol::V1_0
                },
                options: vec![
                    ("CSeq".to_string(), "1".to_string()),
                    ("X-GS-ClientVersion".to_string(), "14".to_string())
                ],
                payload: None
            }
        ),
        "DESCRIBE rtsp://127.0.0.1:80 RTSP/1.0\r\nCSeq: 1\r\nX-GS-ClientVersion: 14\r\n\r\n"
    );
    assert_eq!(
        format!(
            "{}",
            RtspRequest {
                message: RtspRequestMessage {
                    command: RtspCommand::Describe,
                    target: "streamid=video".to_string(),
                    protocol: RtspProtocol::V1_0
                },
                options: vec![("CSeq".to_string(), "1".to_string())],
                payload: Some("a=fmtp:97 surround-params=21101".to_string())
            }
        ),
        "DESCRIBE streamid=video RTSP/1.0\r\nCSeq: 1\r\nContent-Length: 31\r\n\r\na=fmtp:97 surround-params=21101"
    );
}

#[test]
fn rtsp_response_message() {
    let assert_message_eq = |text: &str, message: RtspResponseMessage| {
        assert_eq!(RtspResponseMessage::from_str(text).unwrap(), message);
        assert_eq!(message.to_string(), text);
    };

    assert_message_eq(
        "RTSP/1.0 200 OK",
        RtspResponseMessage {
            protocol: RtspProtocol::V1_0,
            status_code: 200,
            status_message: "OK".to_string(),
        },
    );
    assert_message_eq(
        "RTSP/1.0 404 Not Found",
        RtspResponseMessage {
            protocol: RtspProtocol::V1_0,
            status_code: 404,
            status_message: "Not Found".to_string(),
        },
    );
}

#[test]
#[should_panic]
fn rtsp_response_message_panic() {
    RtspResponseMessage {
        protocol: RtspProtocol::V1_0,
        status_code: 400,
        status_message: "Invalid Status Message\r\n".to_string(),
    }
    .to_string();
}

#[test]
fn rtsp_response() {
    let assert_response_eq = |text: &str, response: RtspResponse, size: usize| {
        assert_eq!(response.to_string(), text);
        assert_eq!(
            RtspResponse::try_parse_header(text).unwrap().unwrap(),
            (size, response)
        );
    };

    assert_response_eq(
        "RTSP/1.0 200 OK\r\nCSeq: 1\r\n\r\n",
        RtspResponse {
            message: RtspResponseMessage {
                protocol: RtspProtocol::V1_0,
                status_code: 200,
                status_message: "OK".to_string(),
            },
            options: vec![("CSeq".to_string(), "1".to_string())],
            payload: None,
        },
        28,
    );

    assert_eq!(
        RtspResponse::try_parse_header(
            "RTSP/1.0 200 OK\r\nCSeq: 2\r\n Content-Length: 31  \r\n\r\na=fmtp:97 surround-params=21101"
        ).unwrap().unwrap(),
        (
            51,
            RtspResponse {
            message: RtspResponseMessage {
                protocol: RtspProtocol::V1_0,
                status_code: 200,
                status_message: "OK".to_string(),
            },
            options: vec![
                ("CSeq".to_string(), "2".to_string()),
                ("Content-Length".to_string(), "31".to_string()),
            ],
            payload: None,
            }
        ),
    );

    assert_eq!(
        &RtspResponse {
            message: RtspResponseMessage {
                protocol: RtspProtocol::V1_0,
                status_code: 200,
                status_message: "OK".to_string(),
            },
            options: vec![
                ("CSeq".to_string(), "2".to_string()),
                ("Content-Length".to_string(), "31".to_string()),
            ],
            payload: Some("a=fmtp:97 surround-params=21101".to_string()),
        }
        .to_string(),
        "RTSP/1.0 200 OK\r\nCSeq: 2\r\nContent-Length: 31\r\n\r\na=fmtp:97 surround-params=21101"
    );
}

#[test]
fn rtsp_send_receive() {
    let mut rtsp = RtspClient::new_unencrypted(RtspClientConfig {
        target: "rtsp://192.168.178.140:48010".parse().unwrap(),
        client_version: 14,
        encryption: None,
    });

    let request = RtspRequest {
        message: RtspRequestMessage {
            command: RtspCommand::Announce,
            target: "rtsp://192.168.178.140:48010".to_string(),
            protocol: RtspProtocol::V1_0,
        },
        options: vec![
            ("Test".to_string(), "1".to_string()),
            ("Test2".to_string(), "2".to_string()),
        ],
        payload: Some("Some Value".to_string()),
    };

    let response = RtspResponse {
        message: RtspResponseMessage {
            protocol: RtspProtocol::V1_0,
            status_code: 200,
            status_message: "Ok".to_string(),
        },
        options: vec![("CSeq".to_string(), "1".to_string())],
        payload: Some("Test".to_string()),
    };

    let mut full_request = request.clone();
    full_request
        .options
        .push(("CSeq".to_string(), "1".to_string()));
    full_request
        .options
        .push(("X-GS-ClientVersion".to_string(), "14".to_string()));
    full_request
        .options
        .push(("Host".to_string(), "192.168.178.140:48010".to_string()));

    assert_eq!(rtsp.poll_output().unwrap(), RtspOutput::Timeout);

    rtsp.send(request);
    assert_eq!(
        rtsp.poll_output().unwrap(),
        RtspOutput::Connect {
            addr: SocketAddrV4::new(Ipv4Addr::new(192, 168, 178, 140), 48010).into(),
        }
    );
    assert_eq!(rtsp.poll_output().unwrap(), RtspOutput::Timeout);

    rtsp.handle_input(RtspInput::Connect).unwrap();
    assert_eq!(
        rtsp.poll_output().unwrap(),
        RtspOutput::Write {
            data: full_request.to_string().into_bytes()
        }
    );
    assert_eq!(rtsp.poll_output().unwrap(), RtspOutput::Timeout);

    rtsp.handle_input(RtspInput::Receive(&response.to_string().into_bytes()))
        .unwrap();
    assert_eq!(rtsp.poll_output().unwrap(), RtspOutput::Timeout);

    rtsp.handle_input(RtspInput::Disconnect).unwrap();
    assert_eq!(rtsp.poll_output().unwrap(), RtspOutput::Response(response));
    assert_eq!(rtsp.poll_output().unwrap(), RtspOutput::Timeout);
}
