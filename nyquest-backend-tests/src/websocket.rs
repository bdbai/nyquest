use std::borrow::Cow;

use futures_util::{SinkExt, StreamExt};
use nyquest::WebSocketMessage;
use tokio::net::TcpListener;
use tokio_tungstenite::{
    accept_hdr_async,
    tungstenite::{
        handshake::server::{Request, Response},
        protocol::{
            frame::{
                coding::{Data, OpCode},
                Frame,
            },
            Message,
        },
    },
};

async fn spawn_server() -> String {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            let (stream, _) = listener.accept().await.unwrap();
            tokio::spawn(async move {
                let callback = |request: &Request, mut response: Response| {
                    if request
                        .headers()
                        .get("Sec-WebSocket-Protocol")
                        .is_some_and(|value| value == "chat")
                    {
                        response
                            .headers_mut()
                            .insert("Sec-WebSocket-Protocol", "chat".parse().unwrap());
                    }
                    Ok(response)
                };
                let Ok(mut websocket) = accept_hdr_async(stream, callback).await else {
                    return;
                };
                while let Some(Ok(message)) = websocket.next().await {
                    match message {
                        Message::Text(text) => {
                            if text == "fragment-server" {
                                let _ = websocket
                                    .send(Message::Frame(Frame::message(
                                        "fragment".as_bytes(),
                                        OpCode::Data(Data::Text),
                                        false,
                                    )))
                                    .await;
                                let _ = websocket
                                    .send(Message::Frame(Frame::message(
                                        "ed".as_bytes(),
                                        OpCode::Data(Data::Continue),
                                        true,
                                    )))
                                    .await;
                                continue;
                            }
                            if text == "close-server" {
                                let _ = websocket
                                    .send(Message::Close(Some(
                                        tokio_tungstenite::tungstenite::protocol::CloseFrame {
                                            code: 1001u16.into(),
                                            reason: "server shutdown".into(),
                                        },
                                    )))
                                    .await;
                                break;
                            }
                            let _ = websocket.send(Message::Text(text)).await;
                        }
                        Message::Binary(bytes) => {
                            let _ = websocket.send(Message::Binary(bytes)).await;
                        }
                        Message::Close(frame) => {
                            let _ = websocket.send(Message::Close(frame)).await;
                            break;
                        }
                        Message::Ping(payload) => {
                            let _ = websocket.send(Message::Pong(payload)).await;
                        }
                        Message::Pong(_) | Message::Frame(_) => {}
                    }
                }
            });
        }
    });
    format!("ws://127.0.0.1:{port}")
}

fn init_ws_backend() {
    crate::init_backend_once();
}

fn assert_close(data: WebSocketMessage, code: u16, reason: &str) {
    assert_eq!(
        data,
        WebSocketMessage::Close {
            code,
            reason: reason.into()
        }
    );
}

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
        .connect_websocket(Cow::Owned(uri), Some(Cow::Borrowed("chat")))
        .await
        .unwrap();

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
        .connect_websocket(Cow::Owned(uri), Some(Cow::Borrowed("chat")))
        .unwrap();

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

#[cfg(feature = "blocking")]
#[test]
fn websocket_blocking_handles_close_frames() {
    init_ws_backend();
    let client = nyquest::ClientBuilder::default().build_blocking().unwrap();
    let mut websocket = client
        .connect_websocket(Cow::Owned(crate::TOKIO_RT.block_on(spawn_server())), None)
        .unwrap();
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
    assert!(matches!(
        websocket.receiver.receive(),
        Err(nyquest::WebSocketError::MessageTooLarge) | Err(nyquest::WebSocketError::Protocol(_))
    ));
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
    assert!(matches!(
        websocket.receiver.receive().await,
        Err(nyquest::WebSocketError::MessageTooLarge) | Err(nyquest::WebSocketError::Protocol(_))
    ));
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
        .connect_websocket(Cow::Owned(crate::TOKIO_RT.block_on(spawn_server())), None)
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
