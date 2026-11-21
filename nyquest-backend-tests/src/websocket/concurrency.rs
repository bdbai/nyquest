#[cfg(feature = "blocking")]
use std::borrow::Cow;

#[cfg(feature = "async")]
use futures_util::{SinkExt, StreamExt};
use nyquest::WebSocketMessage;

use super::{init_ws_backend, spawn_server};

#[cfg(feature = "blocking")]
#[test]
fn websocket_blocking_concurrent_send_while_receiving() {
    use std::sync::{mpsc, Arc, Barrier};
    use std::time::Duration;

    init_ws_backend();
    let client = nyquest::ClientBuilder::default().build_blocking().unwrap();
    let websocket = client
        .connect_websocket(Cow::Owned(crate::TOKIO_RT.block_on(spawn_server())), None)
        .unwrap();
    let mut sender = websocket.sender;
    let mut receiver = websocket.receiver;

    let barrier = Arc::new(Barrier::new(2));
    let (done_tx, done_rx) = mpsc::channel();

    let barrier_recv = barrier.clone();
    let done_tx_recv = done_tx.clone();
    std::thread::spawn(move || {
        barrier_recv.wait();
        let msg = receiver.receive().unwrap();
        let _ = done_tx_recv.send(("recv", Some(msg)));
    });

    std::thread::spawn(move || {
        barrier.wait();
        // Give the receiver thread a moment to enter blocking `receive()`
        std::thread::sleep(Duration::from_millis(100));
        sender
            .send(WebSocketMessage::Utf8("concurrent-hello".into()))
            .unwrap();
        let _ = done_tx.send(("send", None));
    });

    let mut received_msg = None;
    for _ in 0..2 {
        let (who, msg) = done_rx
            .recv_timeout(Duration::from_secs(3))
            .expect("deadlock detected: concurrent blocking send/receive timed out");
        if who == "recv" {
            received_msg = msg;
        }
    }
    assert_eq!(
        received_msg,
        Some(WebSocketMessage::Utf8("concurrent-hello".into()))
    );
}

#[cfg(feature = "async")]
#[tokio::test]
async fn websocket_async_concurrent_send_while_receiving() {
    use std::time::Duration;

    init_ws_backend();
    let client = nyquest::ClientBuilder::default()
        .build_async()
        .await
        .unwrap();
    let uri = format!("{}/echo", spawn_server().await);
    let websocket = client.connect_websocket(uri, None).await.unwrap();
    let mut sender = websocket.sender;
    let mut receiver = websocket.receiver;

    let recv_task = tokio::spawn(async move { receiver.next().await.unwrap().unwrap() });

    tokio::time::sleep(Duration::from_millis(100)).await;
    tokio::time::timeout(
        Duration::from_secs(3),
        sender.send(WebSocketMessage::Utf8("concurrent-async-hello".into())),
    )
    .await
    .expect("deadlock detected: async send timed out while receive was in flight")
    .unwrap();

    let msg = tokio::time::timeout(Duration::from_secs(3), recv_task)
        .await
        .expect("deadlock detected: async receive timed out")
        .unwrap();
    assert_eq!(msg, WebSocketMessage::Utf8("concurrent-async-hello".into()));
}

#[cfg(feature = "async")]
#[tokio::test]
async fn websocket_async_cancel_receive_does_not_lose_message() {
    use std::time::Duration;

    init_ws_backend();
    let client = nyquest::ClientBuilder::default()
        .build_async()
        .await
        .unwrap();
    let uri = format!("{}/echo", spawn_server().await);
    let mut websocket = client.connect_websocket(uri, None).await.unwrap();

    // Start a receive and cancel (drop) its future before any message arrives.
    let timed_out =
        tokio::time::timeout(Duration::from_millis(100), websocket.receiver.next()).await;
    assert!(
        timed_out.is_err(),
        "receive should time out when no message is sent"
    );

    // Now send two messages and verify the first message is not lost or corrupted.
    websocket
        .sender
        .send(WebSocketMessage::Utf8("msg-after-cancel-1".into()))
        .await
        .unwrap();
    websocket
        .sender
        .send(WebSocketMessage::Utf8("msg-after-cancel-2".into()))
        .await
        .unwrap();

    let first = tokio::time::timeout(Duration::from_secs(3), websocket.receiver.next())
        .await
        .expect("receive timed out after cancelled receive")
        .expect("stream ended after cancelled receive")
        .expect("receive failed after cancelled receive");
    assert_eq!(first, WebSocketMessage::Utf8("msg-after-cancel-1".into()));

    let second = tokio::time::timeout(Duration::from_secs(3), websocket.receiver.next())
        .await
        .expect("second receive timed out")
        .expect("stream ended on second receive")
        .expect("second receive failed");
    assert_eq!(second, WebSocketMessage::Utf8("msg-after-cancel-2".into()));

    websocket.sender.close().await.unwrap();
}

#[cfg(feature = "async")]
#[test]
fn websocket_async_runtime_agnostic_and_outlives_client() {
    use std::sync::mpsc;
    use std::time::Duration;

    init_ws_backend();
    let uri = crate::TOKIO_RT.block_on(spawn_server());

    let (done_tx, done_rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut websocket = futures::executor::block_on(async {
            let client = nyquest::ClientBuilder::default()
                .build_async()
                .await
                .unwrap();
            let ws = client
                .connect_websocket(format!("{uri}/echo"), None)
                .await
                .unwrap();
            drop(client);
            ws
        });

        let res = std::thread::spawn(move || {
            futures::executor::block_on(async move {
                websocket
                    .sender
                    .send(WebSocketMessage::Utf8("from-futures-executor".into()))
                    .await?;
                let msg = websocket.receiver.next().await.unwrap()?;
                websocket.sender.close().await?;
                Ok::<_, nyquest::WebSocketError>(msg)
            })
        })
        .join()
        .unwrap();
        let _ = done_tx.send(res);
    });

    let msg = done_rx
        .recv_timeout(Duration::from_secs(3))
        .expect("deadlock detected: async websocket outside tokio timed out")
        .expect("async websocket outside tokio failed after dropping client");
    assert_eq!(msg, WebSocketMessage::Utf8("from-futures-executor".into()));
}

#[cfg(feature = "blocking")]
#[test]
fn websocket_blocking_across_tokio_and_plain_threads() {
    use std::sync::{mpsc, Arc, Barrier};
    use std::time::Duration;

    init_ws_backend();
    let client = nyquest::ClientBuilder::default().build_blocking().unwrap();
    let uri = crate::TOKIO_RT.block_on(spawn_server());

    // Connect on a plain OS thread (outside any Tokio runtime context).
    let websocket = client.connect_websocket(uri, None).unwrap();
    let mut sender = websocket.sender;
    let mut receiver = websocket.receiver;

    let barrier = Arc::new(Barrier::new(2));
    let (done_tx, done_rx) = mpsc::channel();

    // Receive on a thread that HAS a Tokio runtime context (`TOKIO_RT`).
    let barrier_recv = barrier.clone();
    let done_tx_recv = done_tx.clone();
    crate::TOKIO_RT.spawn_blocking(move || {
        barrier_recv.wait();
        let msg = receiver.receive().unwrap();
        let _ = done_tx_recv.send(("recv", Some(msg)));
    });

    // Send from a plain OS thread after receiver is already waiting.
    std::thread::spawn(move || {
        barrier.wait();
        std::thread::sleep(Duration::from_millis(100));
        sender
            .send(WebSocketMessage::Utf8("cross-runtime-hello".into()))
            .unwrap();
        let _ = done_tx.send(("send", None));
    });

    let mut received_msg = None;
    for _ in 0..2 {
        let (who, msg) = done_rx
            .recv_timeout(Duration::from_secs(3))
            .expect("deadlock detected: blocking websocket across tokio/plain threads timed out");
        if who == "recv" {
            received_msg = msg;
        }
    }
    assert_eq!(
        received_msg,
        Some(WebSocketMessage::Utf8("cross-runtime-hello".into()))
    );
}
