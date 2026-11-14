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

mod basic;
mod client_options;
mod concurrency;

pub(super) async fn spawn_server() -> String {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            let (stream, _) = listener.accept().await.unwrap();
            tokio::spawn(async move {
                let mut negotiated_protocols: Option<Vec<String>> = None;
                let mut handshake_headers = hyper::HeaderMap::new();
                #[allow(clippy::result_large_err)]
                let callback = |request: &Request, mut response: Response| {
                    handshake_headers = request.headers().clone();
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
                            if let Some(header_name) = text.strip_prefix("get-header:") {
                                let reply = handshake_headers
                                    .get(header_name)
                                    .and_then(|v| v.to_str().ok())
                                    .unwrap_or_default()
                                    .to_owned();
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

pub(super) fn init_ws_backend() {
    crate::init_backend_once();
}

pub(super) fn assert_close(data: WebSocketMessage, code: u16, reason: &str) {
    assert_eq!(
        data,
        WebSocketMessage::Close {
            code,
            reason: reason.into()
        }
    );
}
