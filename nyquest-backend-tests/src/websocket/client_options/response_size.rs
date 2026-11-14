use std::borrow::Cow;

use nyquest::WebSocketMessage;

use crate::websocket::{init_ws_backend, spawn_server};

#[test]
fn websocket_client_options_message_within_limit() {
    const BODY: &str = "1234567890"; // 10 bytes
    init_ws_backend();
    let uri = crate::TOKIO_RT.block_on(spawn_server());

    #[cfg(feature = "blocking")]
    {
        let client = nyquest::ClientBuilder::default()
            .max_response_buffer_size(10)
            .build_blocking()
            .unwrap();
        let mut ws = client
            .connect_websocket(format!("{uri}/limit"), None)
            .unwrap();
        ws.sender.send(WebSocketMessage::Utf8(BODY.into())).unwrap();
        assert_eq!(
            ws.receiver.receive().unwrap(),
            WebSocketMessage::Utf8(BODY.into())
        );
        ws.sender.close().unwrap();
    }

    #[cfg(feature = "async")]
    crate::TOKIO_RT.block_on(async {
        let client = nyquest::ClientBuilder::default()
            .max_response_buffer_size(10)
            .build_async()
            .await
            .unwrap();
        let mut ws = client
            .connect_websocket(format!("{uri}/limit"), None)
            .await
            .unwrap();
        ws.sender
            .send(WebSocketMessage::Utf8(BODY.into()))
            .await
            .unwrap();
        assert_eq!(
            ws.receiver.receive().await.unwrap(),
            WebSocketMessage::Utf8(BODY.into())
        );
        ws.sender.close().await.unwrap();
    });
}

#[cfg(feature = "blocking")]
#[test]
fn websocket_blocking_enforces_message_limit() {
    init_ws_backend();
    let client = nyquest::ClientBuilder::default()
        .max_response_buffer_size(4)
        .build_blocking()
        .unwrap();
    let mut websocket = client
        .connect_websocket(Cow::Owned(crate::TOKIO_RT.block_on(spawn_server())), None)
        .unwrap();
    websocket
        .sender
        .send(WebSocketMessage::Utf8("too large".into()))
        .unwrap();
    assert!(websocket.receiver.receive().is_err());
}

#[cfg(feature = "async")]
#[tokio::test]
async fn websocket_async_enforces_message_limit() {
    init_ws_backend();
    let client = nyquest::ClientBuilder::default()
        .max_response_buffer_size(4)
        .build_async()
        .await
        .unwrap();
    let mut websocket = client
        .connect_websocket(Cow::Owned(format!("{}/echo", spawn_server().await)), None)
        .await
        .unwrap();
    websocket
        .sender
        .send(WebSocketMessage::Utf8("too large".into()))
        .await
        .unwrap();
    assert!(websocket.receiver.receive().await.is_err());
}
