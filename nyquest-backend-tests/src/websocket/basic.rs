use std::borrow::Cow;

#[cfg(feature = "async")]
use futures_util::{SinkExt, StreamExt};
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
        websocket.receiver.next().await.unwrap().unwrap(),
        WebSocketMessage::Utf8("hello".into())
    );

    websocket
        .sender
        .send(WebSocketMessage::Bytes(vec![1, 2, 3]))
        .await
        .unwrap();
    assert_eq!(
        websocket.receiver.next().await.unwrap().unwrap(),
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
        websocket.receiver.next().await.unwrap().unwrap(),
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
        websocket.receiver.next().await.unwrap().unwrap(),
        1001,
        "server shutdown",
    );
    // After the close frame the stream is terminated.
    assert!(websocket.receiver.next().await.is_none());
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
        websocket.receiver.next().await.unwrap().unwrap(),
        WebSocketMessage::Utf8("fragmented".into())
    );
}

#[cfg(feature = "async")]
#[tokio::test]
async fn websocket_async_explicit_close_then_close_is_idempotent() {
    init_ws_backend();
    let client = nyquest::ClientBuilder::default()
        .build_async()
        .await
        .unwrap();
    let uri = format!("{}/echo", spawn_server().await);
    let mut websocket = client.connect_websocket(uri, None).await.unwrap();

    // Buffer an explicit close frame without flushing it, then close the sink.
    // `poll_close` must flush the pending close instead of emitting a second one.
    websocket
        .sender
        .feed(WebSocketMessage::Close {
            code: 1000,
            reason: "done".into(),
        })
        .await
        .unwrap();
    websocket.sender.close().await.unwrap();
    // A second close is a no-op and must not fail or emit another frame.
    websocket.sender.close().await.unwrap();

    // Sending after close initiation is rejected.
    assert!(websocket
        .sender
        .feed(WebSocketMessage::Bytes(vec![1, 2, 3]))
        .await
        .is_err());

    // The stream terminates: at most one close frame is yielded (the peer may
    // or may not echo ours before going away), then `None`.
    let mut saw_close = false;
    while let Some(res) = websocket.receiver.next().await {
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
