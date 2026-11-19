#![cfg(all(target_arch = "wasm32", feature = "async-ws"))]

use std::borrow::Cow;

use futures::{SinkExt, StreamExt};
use nyquest_backend_reqwest::ReqwestBackend;
use nyquest_interface::{
    client::ClientOptions,
    r#async::AnyAsyncBackend,
    websocket::{WebSocketError, WebSocketMessage},
};
use wasm_bindgen_test::wasm_bindgen_test;

wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

const ECHO_URL: &str = "wss://echo.websocket.org/";

async fn connect_echo(
    options: ClientOptions,
    uri: Cow<'static, str>,
    protocols: Vec<Cow<'static, str>>,
) -> nyquest_interface::r#async::websocket::WebSocket {
    let backend = ReqwestBackend::default();
    let client = backend.create_async_client(options).await.unwrap();
    client.connect_websocket(uri, protocols).await.unwrap()
}

#[wasm_bindgen_test]
async fn websocket_echoes_text_and_binary() {
    let mut ws = connect_echo(ClientOptions::default(), ECHO_URL.into(), vec![]).await;

    ws.sender
        .send(WebSocketMessage::Utf8("hello".into()))
        .await
        .unwrap();
    assert_eq!(
        ws.receiver.next().await.unwrap().unwrap(),
        WebSocketMessage::Utf8("hello".into())
    );

    ws.sender
        .send(WebSocketMessage::Bytes(vec![1, 2, 3]))
        .await
        .unwrap();
    assert_eq!(
        ws.receiver.next().await.unwrap().unwrap(),
        WebSocketMessage::Bytes(vec![1, 2, 3])
    );

    ws.sender.close().await.unwrap();
}

#[wasm_bindgen_test]
async fn websocket_rejects_invalid_url() {
    let backend = ReqwestBackend::default();
    let client = backend
        .create_async_client(ClientOptions::default())
        .await
        .unwrap();
    let err = client
        .connect_websocket("not a url".into(), vec![])
        .await
        .unwrap_err();
    assert!(matches!(err, WebSocketError::InvalidUrl));

    let err = client
        .connect_websocket("foo://example.com/ws".into(), vec![])
        .await
        .unwrap_err();
    assert!(matches!(err, WebSocketError::InvalidUrl));
}

#[wasm_bindgen_test]
async fn websocket_explicit_close_then_close_is_idempotent() {
    let mut ws = connect_echo(ClientOptions::default(), ECHO_URL.into(), vec![]).await;

    // Buffer an explicit close frame, then close the sink. poll_close must
    // flush the pending close instead of emitting a second one.
    ws.sender
        .feed(WebSocketMessage::Close {
            code: 1000,
            reason: "done".into(),
        })
        .await
        .unwrap();
    ws.sender.close().await.unwrap();
    // A second close is a no-op and must not fail or emit another frame.
    ws.sender.close().await.unwrap();

    // Sending after close initiation is rejected.
    assert!(ws
        .sender
        .feed(WebSocketMessage::Bytes(vec![1, 2, 3]))
        .await
        .is_err());

    // The stream terminates: at most one close frame is yielded, then None.
    let mut saw_close = false;
    while let Some(res) = ws.receiver.next().await {
        match res {
            Ok(WebSocketMessage::Close { .. }) => {
                assert!(!saw_close, "close frame yielded more than once");
                saw_close = true;
            }
            Err(_) => {}
            Ok(other) => panic!("unexpected message after close: {other:?}"),
        }
    }
}

#[wasm_bindgen_test]
async fn websocket_connects_with_multiple_protocols() {
    // echo.websocket.org may not negotiate a sub-protocol; the point here is
    // exercising the multi-protocol (js_sys::Array) open path.
    let mut ws = connect_echo(
        ClientOptions::default(),
        ECHO_URL.into(),
        vec!["chat".into(), "superchat".into()],
    )
    .await;
    if let Some(protocol) = ws.info.protocol.as_deref() {
        assert!(protocol == "chat" || protocol == "superchat");
    }

    ws.sender
        .send(WebSocketMessage::Utf8("proto-echo".into()))
        .await
        .unwrap();
    assert_eq!(
        ws.receiver.next().await.unwrap().unwrap(),
        WebSocketMessage::Utf8("proto-echo".into())
    );
    ws.sender.close().await.unwrap();
}

#[wasm_bindgen_test]
async fn websocket_respects_max_message_size() {
    let options = ClientOptions {
        max_response_buffer_size: Some(2),
        ..Default::default()
    };
    let mut ws = connect_echo(options, ECHO_URL.into(), vec![]).await;

    ws.sender
        .send(WebSocketMessage::Utf8("hello".into()))
        .await
        .unwrap();
    let res = ws.receiver.next().await.unwrap();
    assert!(matches!(res, Err(WebSocketError::MessageTooLarge)));
}

#[wasm_bindgen_test]
async fn websocket_supports_base_url() {
    // https base URL must be converted to wss for the browser WebSocket API.
    let options = ClientOptions {
        base_url: Some("https://echo.websocket.org".into()),
        ..Default::default()
    };
    let mut ws = connect_echo(options, Cow::Borrowed("/"), vec![]).await;

    ws.sender
        .send(WebSocketMessage::Utf8("base-url".into()))
        .await
        .unwrap();
    assert_eq!(
        ws.receiver.next().await.unwrap().unwrap(),
        WebSocketMessage::Utf8("base-url".into())
    );
    ws.sender.close().await.unwrap();
}
