use std::borrow::Cow;

use nyquest::WebSocketMessage;

use super::{assert_close, init_ws_backend, spawn_server};

#[test]
fn websocket_blocking_iter_echoes_with_helpers() {
    init_ws_backend();
    let client = nyquest::ClientBuilder::default().build_blocking().unwrap();
    let uri = crate::TOKIO_RT.block_on(spawn_server());
    let mut ws = client
        .connect_websocket(uri, [Cow::Borrowed("chat")])
        .unwrap();
    assert_eq!(ws.info.protocol.as_deref(), Some("chat"));

    ws.sender.send(WebSocketMessage::text("hello")).unwrap();
    ws.sender.send(WebSocketMessage::binary([1, 2, 3])).unwrap();
    // Move the receiver into its owning iterator; sender is no longer needed.
    let mut iter = ws.receiver.into_iter();

    let msg = iter.next().expect("first echo").unwrap();
    assert!(msg.is_text());
    assert!(!msg.is_binary());
    assert!(!msg.is_close());
    assert_eq!(msg.to_text().unwrap(), "hello");

    let msg = iter.next().expect("second echo").unwrap();
    assert!(msg.is_binary());
    assert_eq!(msg.into_data(), vec![1, 2, 3]);
    // Dropping the iterator here must not hang: we simply stop polling.
}

#[test]
fn websocket_blocking_iter_fused_after_close() {
    init_ws_backend();
    let client = nyquest::ClientBuilder::default().build_blocking().unwrap();
    let mut ws = client
        .connect_websocket(Cow::Owned(crate::TOKIO_RT.block_on(spawn_server())), None)
        .unwrap();
    ws.sender
        .send(WebSocketMessage::text("close-server"))
        .unwrap();
    // `ws.sender` is dropped here; only the iterator drives the connection.
    let mut iter = ws.receiver.into_iter();

    let msg = iter.next().expect("close frame").unwrap();
    assert!(msg.is_close());
    assert_close(msg, 1001, "server shutdown");

    // Fused: every subsequent `next()` returns `None`.
    assert!(iter.next().is_none());
    assert!(iter.next().is_none());
    assert!(iter.next().is_none());
}

#[test]
fn websocket_blocking_iter_fused_after_error() {
    init_ws_backend();
    let client = nyquest::ClientBuilder::default()
        .max_response_buffer_size(4)
        .build_blocking()
        .unwrap();
    let mut ws = client
        .connect_websocket(Cow::Owned(crate::TOKIO_RT.block_on(spawn_server())), None)
        .unwrap();
    ws.sender.send(WebSocketMessage::text("too large")).unwrap();
    let mut iter = ws.receiver.into_iter();

    let err = iter.next().expect("error item");
    assert!(err.is_err());
    // Fused after the error item: no further items even on repeated polling.
    assert!(iter.next().is_none());
    assert!(iter.next().is_none());
}
