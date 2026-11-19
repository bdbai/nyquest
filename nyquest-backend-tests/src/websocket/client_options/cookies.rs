#[cfg(feature = "async")]
use futures_util::{SinkExt, StreamExt};
use http_body_util::Full;
use hyper::header::SET_COOKIE;
use hyper::{body::Bytes, Response};
use nyquest::{Request as NyquestRequest, WebSocketMessage};

use crate::websocket::{init_ws_backend, spawn_server};

#[test]
fn websocket_client_options_cookies_enabled() {
    const PATH: &str = "client_options/ws_cookies_enabled";
    const COOKIE_VALUE: &str = "ws_cookie_enabled_val";

    init_ws_backend();
    let _handle = crate::add_hyper_fixture(PATH, move |_req| async move {
        let res = Response::builder()
            .header(SET_COOKIE, format!("WS_COOKIE={COOKIE_VALUE}; Path=/"))
            .body(Full::new(Bytes::from_static(b"ok")))
            .unwrap();
        (res, Ok(()))
    });
    let ws_uri = crate::TOKIO_RT
        .block_on(spawn_server())
        .replace("127.0.0.1", "localhost.");

    #[cfg(feature = "blocking")]
    {
        let client = crate::init_builder_blocking()
            .unwrap()
            .build_blocking()
            .unwrap();
        client.request(NyquestRequest::get(PATH)).unwrap();

        let mut ws = client
            .connect_websocket(format!("{ws_uri}/cookies"), None)
            .unwrap();
        ws.sender
            .send(WebSocketMessage::Utf8("get-header:cookie".into()))
            .unwrap();
        let WebSocketMessage::Utf8(cookie_header) = ws.receiver.receive().unwrap() else {
            panic!("expected text message");
        };
        assert!(
            cookie_header.contains(&format!("WS_COOKIE={COOKIE_VALUE}")),
            "expected cookie in handshake header, got: {cookie_header:?}"
        );
        ws.sender.close().unwrap();
    }

    #[cfg(feature = "async")]
    crate::TOKIO_RT.block_on(async {
        let client = crate::init_builder()
            .await
            .unwrap()
            .build_async()
            .await
            .unwrap();
        client.request(NyquestRequest::get(PATH)).await.unwrap();

        let mut ws = client
            .connect_websocket(format!("{ws_uri}/cookies"), None)
            .await
            .unwrap();
        ws.sender
            .send(WebSocketMessage::Utf8("get-header:cookie".into()))
            .await
            .unwrap();
        let WebSocketMessage::Utf8(cookie_header) = ws.receiver.next().await.unwrap().unwrap()
        else {
            panic!("expected text message");
        };
        assert!(
            cookie_header.contains(&format!("WS_COOKIE={COOKIE_VALUE}")),
            "expected cookie in handshake header, got: {cookie_header:?}"
        );
        ws.sender.close().await.unwrap();
    });
}

#[test]
fn websocket_client_options_cookies_disabled() {
    const PATH: &str = "client_options/ws_cookies_disabled";
    const COOKIE_VALUE: &str = "ws_cookie_disabled_val";

    init_ws_backend();
    let _handle = crate::add_hyper_fixture(PATH, move |_req| async move {
        let res = Response::builder()
            .header(
                SET_COOKIE,
                format!("WS_DISABLED_COOKIE={COOKIE_VALUE}; Path=/"),
            )
            .body(Full::new(Bytes::from_static(b"ok")))
            .unwrap();
        (res, Ok(()))
    });
    let ws_uri = crate::TOKIO_RT
        .block_on(spawn_server())
        .replace("127.0.0.1", "localhost.");

    #[cfg(feature = "blocking")]
    {
        let client = crate::init_builder_blocking()
            .unwrap()
            .no_cookies()
            .build_blocking()
            .unwrap();
        client.request(NyquestRequest::get(PATH)).unwrap();

        let mut ws = client
            .connect_websocket(format!("{ws_uri}/cookies"), None)
            .unwrap();
        ws.sender
            .send(WebSocketMessage::Utf8("get-header:cookie".into()))
            .unwrap();
        let WebSocketMessage::Utf8(cookie_header) = ws.receiver.receive().unwrap() else {
            panic!("expected text message");
        };
        assert!(
            !cookie_header.contains("WS_DISABLED_COOKIE"),
            "expected no WS_DISABLED_COOKIE in handshake header, got: {cookie_header:?}"
        );
        ws.sender.close().unwrap();
    }

    #[cfg(feature = "async")]
    crate::TOKIO_RT.block_on(async {
        let client = crate::init_builder()
            .await
            .unwrap()
            .no_cookies()
            .build_async()
            .await
            .unwrap();
        client.request(NyquestRequest::get(PATH)).await.unwrap();

        let mut ws = client
            .connect_websocket(format!("{ws_uri}/cookies"), None)
            .await
            .unwrap();
        ws.sender
            .send(WebSocketMessage::Utf8("get-header:cookie".into()))
            .await
            .unwrap();
        let WebSocketMessage::Utf8(cookie_header) = ws.receiver.next().await.unwrap().unwrap()
        else {
            panic!("expected text message");
        };
        assert!(
            !cookie_header.contains("WS_DISABLED_COOKIE"),
            "expected no WS_DISABLED_COOKIE in handshake header, got: {cookie_header:?}"
        );
        ws.sender.close().await.unwrap();
    });
}
