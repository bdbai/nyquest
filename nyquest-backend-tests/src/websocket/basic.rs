use std::borrow::Cow;

use nyquest::WebSocketMessage;

use super::{assert_close, init_ws_backend, spawn_server};

#[cfg(feature = "async")]
#[tokio::test]
async fn websocket_async_echoes_text_and_binary() {
    init_ws_backend();
    let client = nyquest::ClientBuilder::default()
        .build_async()
        .await
        .unwrap();
    let uri = format!("{}/echo", spawn_server().await);
    let mut websocket = client
        .connect_websocket(uri, [Cow::Borrowed("chat")])
        .await
        .unwrap();
    assert_eq!(websocket.info.protocol.as_deref(), Some("chat"));

    websocket
        .sender
        .send(WebSocketMessage::Utf8("hello".into()))
        .await
        .unwrap();
    assert_eq!(
        websocket.receiver.receive().await.unwrap(),
        WebSocketMessage::Utf8("hello".into())
    );

    websocket
        .sender
        .send(WebSocketMessage::Bytes(vec![1, 2, 3]))
        .await
        .unwrap();
    assert_eq!(
        websocket.receiver.receive().await.unwrap(),
        WebSocketMessage::Bytes(vec![1, 2, 3])
    );
    websocket.sender.close().await.unwrap();
}

#[cfg(feature = "blocking")]
#[test]
fn websocket_blocking_echoes_text_and_binary() {
    init_ws_backend();
    let client = nyquest::ClientBuilder::default().build_blocking().unwrap();
    let uri = crate::TOKIO_RT.block_on(spawn_server());
    let mut websocket = client
        .connect_websocket(uri, [Cow::Borrowed("chat")])
        .unwrap();
    assert_eq!(websocket.info.protocol.as_deref(), Some("chat"));

    websocket
        .sender
        .send(WebSocketMessage::Utf8("hello".into()))
        .unwrap();
    assert_eq!(
        websocket.receiver.receive().unwrap(),
        WebSocketMessage::Utf8("hello".into())
    );
    websocket
        .sender
        .send(WebSocketMessage::Bytes(vec![1, 2, 3]))
        .unwrap();
    assert_eq!(
        websocket.receiver.receive().unwrap(),
        WebSocketMessage::Bytes(vec![1, 2, 3])
    );
    websocket.sender.close().unwrap();
}

#[cfg(feature = "async")]
#[tokio::test]
async fn websocket_async_negotiates_multiple_protocols() {
    init_ws_backend();
    let client = nyquest::ClientBuilder::default()
        .build_async()
        .await
        .unwrap();
    let uri = format!("{}/protocols", spawn_server().await);
    let mut websocket = client
        .connect_websocket(uri, ["proto1".into(), "superchat".into(), "chat".into()])
        .await
        .unwrap();
    assert_eq!(websocket.info.protocol.as_deref(), Some("superchat"));

    websocket
        .sender
        .send(WebSocketMessage::Utf8("get-protocols".into()))
        .await
        .unwrap();
    assert_eq!(
        websocket.receiver.receive().await.unwrap(),
        WebSocketMessage::Utf8("proto1,superchat,chat".into())
    );
    websocket.sender.close().await.unwrap();
}

#[cfg(feature = "blocking")]
#[test]
fn websocket_blocking_negotiates_multiple_protocols() {
    init_ws_backend();
    let client = nyquest::ClientBuilder::default().build_blocking().unwrap();
    let uri = crate::TOKIO_RT.block_on(spawn_server());
    let mut websocket = client
        .connect_websocket(uri, ["proto1".into(), "superchat".into(), "chat".into()])
        .unwrap();
    assert_eq!(websocket.info.protocol.as_deref(), Some("superchat"));

    websocket
        .sender
        .send(WebSocketMessage::Utf8("get-protocols".into()))
        .unwrap();
    assert_eq!(
        websocket.receiver.receive().unwrap(),
        WebSocketMessage::Utf8("proto1,superchat,chat".into())
    );
    websocket.sender.close().unwrap();
}

#[cfg(feature = "blocking")]
#[test]
fn websocket_blocking_handles_close_frames() {
    init_ws_backend();
    let client = nyquest::ClientBuilder::default().build_blocking().unwrap();
    let mut websocket = client
        .connect_websocket(Cow::Owned(crate::TOKIO_RT.block_on(spawn_server())), None)
        .unwrap();
    assert_eq!(websocket.info.protocol, None);
    websocket
        .sender
        .send(WebSocketMessage::Utf8("close-server".into()))
        .unwrap();
    assert_close(
        websocket.receiver.receive().unwrap(),
        1001,
        "server shutdown",
    );
}

#[cfg(feature = "async")]
#[tokio::test]
async fn websocket_async_handles_close_frames() {
    init_ws_backend();
    let client = nyquest::ClientBuilder::default()
        .build_async()
        .await
        .unwrap();
    let mut websocket = client
        .connect_websocket(Cow::Owned(format!("{}/close", spawn_server().await)), None)
        .await
        .unwrap();
    assert_eq!(websocket.info.protocol, None);
    websocket
        .sender
        .send(WebSocketMessage::Utf8("close-server".into()))
        .await
        .unwrap();
    assert_close(
        websocket.receiver.receive().await.unwrap(),
        1001,
        "server shutdown",
    );
}

#[cfg(feature = "async")]
#[tokio::test]
async fn websocket_async_reassembles_fragmented_messages() {
    init_ws_backend();
    let client = nyquest::ClientBuilder::default()
        .build_async()
        .await
        .unwrap();
    let mut websocket = client
        .connect_websocket(
            Cow::Owned(format!("{}/fragment", spawn_server().await)),
            None,
        )
        .await
        .unwrap();
    websocket
        .sender
        .send(WebSocketMessage::Utf8("fragment-server".into()))
        .await
        .unwrap();
    assert_eq!(
        websocket.receiver.receive().await.unwrap(),
        WebSocketMessage::Utf8("fragmented".into())
    );
}

#[cfg(feature = "blocking")]
#[test]
fn websocket_blocking_reassembles_fragmented_messages() {
    init_ws_backend();
    let client = nyquest::ClientBuilder::default().build_blocking().unwrap();
    let mut websocket = client
        .connect_websocket(Cow::Owned(crate::TOKIO_RT.block_on(spawn_server())), vec![])
        .unwrap();
    websocket
        .sender
        .send(WebSocketMessage::Utf8("fragment-server".into()))
        .unwrap();
    assert_eq!(
        websocket.receiver.receive().unwrap(),
        WebSocketMessage::Utf8("fragmented".into())
    );
}
