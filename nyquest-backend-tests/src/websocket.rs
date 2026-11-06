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
                let mut negotiated_protocols: Option<Vec<String>> = None;
                #[allow(clippy::result_large_err)]
                let callback = |request: &Request, mut response: Response| {
                    let protocols: Vec<String> = request
                        .headers()
                        .get_all("Sec-WebSocket-Protocol")
                        .iter()
                        .filter_map(|value| value.to_str().ok())
                        .flat_map(|value| value.split(','))
                        .map(|proto| proto.trim().to_owned())
                        .filter(|proto| !proto.is_empty())
                        .collect();
                    if !protocols.is_empty() {
                        if let Some(selected) = protocols
                            .iter()
                            .find(|proto| *proto == "superchat" || *proto == "chat")
                        {
                            response
                                .headers_mut()
                                .insert("Sec-WebSocket-Protocol", selected.parse().unwrap());
                        }
                        negotiated_protocols = Some(protocols);
                    }
                    Ok(response)
                };
                let Ok(mut websocket) = accept_hdr_async(stream, callback).await else {
                    return;
                };
                while let Some(Ok(message)) = websocket.next().await {
                    match message {
                        Message::Text(text) => {
                            if text == "get-protocols" {
                                let reply = negotiated_protocols
                                    .as_ref()
                                    .map(|p| p.join(","))
                                    .unwrap_or_default();
                                let _ = websocket.send(Message::Text(reply.into())).await;
                                continue;
                            }
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
