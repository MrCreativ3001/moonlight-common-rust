use std::assert_matches;

use bytes::Bytes;
use http::{HeaderName, HeaderValue, Method, Request, StatusCode, Uri, Version, header};

use crate::http::client::rustls::{
    HttpsClient, HttpsConfig, deserialize_response, serialize_request,
};

#[test]
fn request_serialization() {
    let request = Request::builder()
        .uri(
            Uri::builder()
                .scheme("http")
                .authority("sunshine:80")
                .path_and_query("/serverinfo?test=test2")
                .build()
                .unwrap(),
        )
        .method(Method::GET)
        .body(Bytes::copy_from_slice("hello payload".as_bytes()))
        .unwrap();

    assert_eq!(
        str::from_utf8(&serialize_request(request)).unwrap(),
        "GET /serverinfo?test=test2 HTTP/1.1\r\nHost: sunshine\r\ncontent-length: 13\r\n\r\nhello payload"
    );
}

#[test]
fn response_deserialize_not_finished() {
    assert_matches!(
        deserialize_response("HTTP/1.1 401 Unauthorized\r\nContent-Type: text/xml\r\nContent-Length: 13\r\n\r\nhello payloa".as_bytes()),
        Ok(None)
    );
}

#[test]
fn response_deserialization() {
    let response_text =
        "HTTP/1.1 401 Unauthorized\r\nTest-Header: test\r\nContent-Type: text/xml\r\nContent-Length: 13\r\n\r\nhello payload"
            .as_bytes();

    let response = deserialize_response(response_text)
        .expect("parse http response")
        .expect("http response is finish");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(response.version(), Version::HTTP_11);
    assert_eq!(
        response
            .headers()
            .get(HeaderName::from_static("test-header")),
        Some(&HeaderValue::from_static("test"))
    );
    assert_eq!(
        response.headers().get(header::CONTENT_TYPE),
        Some(&HeaderValue::from_static("text/xml"))
    );
    assert_eq!(response.body(), "hello payload".as_bytes());
}

#[test]
fn http_request_response() {
    let request = Request::builder()
        .uri(
            Uri::builder()
                .scheme("http")
                .authority("sunshine")
                .path_and_query("/serverinfo")
                .build()
                .unwrap(),
        )
        .body(Bytes::new())
        .unwrap();

    let mut client = HttpsClient::new(&HttpsConfig::http(), request.clone()).unwrap();

    let mut buffer = [0; 1024];
    let len = client.poll_write(&mut buffer).unwrap();
    let buffer = &buffer[0..len];

    assert_eq!(buffer, serialize_request(request).as_slice());

    client.handle_read(
        "HTTP/1.1 401 Unauthorized\r\nTest-Header: test\r\nContent-Type: text/xml\r\nContent-Length: 13\r\n\r\nhello payload".as_bytes()
    ).unwrap();

    let response = client
        .poll_output()
        .expect("parse http response")
        .expect("http response is finished");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(response.body(), "hello payload".as_bytes());
}

#[test]
fn http_request_small_buffer() {
    let request = Request::builder()
        .uri(
            Uri::builder()
                .scheme("http")
                .authority("sunshine")
                .path_and_query("/serverinfo")
                .build()
                .unwrap(),
        )
        .body(Bytes::new())
        .unwrap();

    let mut client = HttpsClient::new(&HttpsConfig::http(), request.clone()).unwrap();

    let mut full = vec![];
    let mut buffer = [0; 10];

    for _ in 0..1000 {
        let len = client.poll_write(&mut buffer).unwrap();
        if len == 0 {
            break;
        }

        full.extend_from_slice(&buffer[0..len]);
    }

    assert_eq!(full.as_slice(), serialize_request(request).as_slice());
}
