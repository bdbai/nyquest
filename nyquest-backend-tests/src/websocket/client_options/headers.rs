#[cfg(feature = "async")]
use futures_util::{SinkExt, StreamExt};
use nyquest::WebSocketMessage;

use crate::websocket::{init_ws_backend, spawn_server};

#[test]
fn websocket_client_options_user_agent() {
    const USER_AGENT_VALUE: &str = "Nyquest/1.0 (WebSocket Test User Agent)";
    init_ws_backend();
    let uri = crate::TOKIO_RT.block_on(spawn_server());

    #[cfg(feature = "blocking")]
    {
        let client = nyquest::ClientBuilder::default()
            .user_agent(USER_AGENT_VALUE)
            .build_blocking()
            .unwrap();
        let mut ws = client.connect_websocket(format!("{uri}/ua"), None).unwrap();
        ws.sender
            .send(WebSocketMessage::Utf8("get-header:user-agent".into()))
            .unwrap();
        assert_eq!(
            ws.receiver.receive().unwrap(),
            WebSocketMessage::Utf8(USER_AGENT_VALUE.into())
        );
        ws.sender.close().unwrap();
    }

    #[cfg(feature = "async")]
    crate::TOKIO_RT.block_on(async {
        let client = nyquest::ClientBuilder::default()
            .user_agent(USER_AGENT_VALUE)
            .build_async()
            .await
            .unwrap();
        let mut ws = client
            .connect_websocket(format!("{uri}/ua"), None)
            .await
            .unwrap();
        ws.sender
            .send(WebSocketMessage::Utf8("get-header:user-agent".into()))
            .await
            .unwrap();
        assert_eq!(
            ws.receiver.next().await.unwrap().unwrap(),
            WebSocketMessage::Utf8(USER_AGENT_VALUE.into())
        );
        ws.sender.close().await.unwrap();
    });
}

#[test]
fn websocket_client_options_default_headers() {
    const HEADER_X_TEST: &str = "custom-ws-value";
    const HEADER_LANG: &str = "zh-CN";
    init_ws_backend();
    let uri = crate::TOKIO_RT.block_on(spawn_server());

    #[cfg(feature = "blocking")]
    {
        let client = nyquest::ClientBuilder::default()
            .with_header("X-Test-Header", HEADER_X_TEST)
            .with_header("Content-Language", HEADER_LANG)
            .build_blocking()
            .unwrap();
        let mut ws = client
            .connect_websocket(format!("{uri}/headers"), None)
            .unwrap();
        ws.sender
            .send(WebSocketMessage::Utf8("get-header:x-test-header".into()))
            .unwrap();
        assert_eq!(
            ws.receiver.receive().unwrap(),
            WebSocketMessage::Utf8(HEADER_X_TEST.into())
        );
        ws.sender
            .send(WebSocketMessage::Utf8("get-header:content-language".into()))
            .unwrap();
        assert_eq!(
            ws.receiver.receive().unwrap(),
            WebSocketMessage::Utf8(HEADER_LANG.into())
        );
        ws.sender.close().unwrap();
    }

    #[cfg(feature = "async")]
    crate::TOKIO_RT.block_on(async {
        let client = nyquest::ClientBuilder::default()
            .with_header("X-Test-Header", HEADER_X_TEST)
            .with_header("Content-Language", HEADER_LANG)
            .build_async()
            .await
            .unwrap();
        let mut ws = client
            .connect_websocket(format!("{uri}/headers"), None)
            .await
            .unwrap();
        ws.sender
            .send(WebSocketMessage::Utf8("get-header:x-test-header".into()))
            .await
            .unwrap();
        assert_eq!(
            ws.receiver.next().await.unwrap().unwrap(),
            WebSocketMessage::Utf8(HEADER_X_TEST.into())
        );
        ws.sender
            .send(WebSocketMessage::Utf8("get-header:content-language".into()))
            .await
            .unwrap();
        assert_eq!(
            ws.receiver.next().await.unwrap().unwrap(),
            WebSocketMessage::Utf8(HEADER_LANG.into())
        );
        ws.sender.close().await.unwrap();
    });
}

#[test]
fn websocket_client_options_base_url() {
    init_ws_backend();
    let base_url =
        format!("{}/base/", crate::TOKIO_RT.block_on(spawn_server())).replace("ws://", "http://");

    #[cfg(feature = "blocking")]
    {
        let client = nyquest::ClientBuilder::default()
            .base_url(base_url.clone())
            .build_blocking()
            .unwrap();
        let mut ws = client.connect_websocket("echo", None).unwrap();
        ws.sender
            .send(WebSocketMessage::Utf8("base-url-msg".into()))
            .unwrap();
        assert_eq!(
            ws.receiver.receive().unwrap(),
            WebSocketMessage::Utf8("base-url-msg".into())
        );
        ws.sender.close().unwrap();
    }

    #[cfg(feature = "async")]
    crate::TOKIO_RT.block_on(async {
        let client = nyquest::ClientBuilder::default()
            .base_url(base_url)
            .build_async()
            .await
            .unwrap();
        let mut ws = client.connect_websocket("echo", None).await.unwrap();
        ws.sender
            .send(WebSocketMessage::Utf8("base-url-msg".into()))
            .await
            .unwrap();
        assert_eq!(
            ws.receiver.next().await.unwrap().unwrap(),
            WebSocketMessage::Utf8("base-url-msg".into())
        );
        ws.sender.close().await.unwrap();
    });
}
