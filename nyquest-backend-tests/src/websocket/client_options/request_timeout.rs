use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use nyquest::WebSocketMessage;
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::protocol::Message;

use crate::websocket::init_ws_backend;

async fn spawn_delayed_ws_server(delay: Duration) -> String {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                break;
            };
            tokio::spawn(async move {
                tokio::time::sleep(delay).await;
                let Ok(mut websocket) = tokio_tungstenite::accept_async(stream).await else {
                    return;
                };
                while let Some(Ok(message)) = websocket.next().await {
                    match message {
                        Message::Text(text) => {
                            let _ = websocket.send(Message::Text(text)).await;
                        }
                        Message::Binary(bytes) => {
                            let _ = websocket.send(Message::Binary(bytes)).await;
                        }
                        Message::Close(frame) => {
                            let _ = websocket.send(Message::Close(frame)).await;
                            break;
                        }
                        _ => {}
                    }
                }
            });
        }
    });
    format!("ws://127.0.0.1:{port}/delayed")
}

#[test]
// libcurl has no way to set a timeout for the websocket handshake:
// blocking: CURLOPT_TIMEOUT_MS has no use when CURLOPT_CONNECT_ONLY=2L
// async: perform() fails when the timeout is reached even if the connection is established
#[cfg(not(feature = "curl"))]
fn websocket_client_options_request_timeout() {
    use std::time::Instant;

    init_ws_backend();
    let slow_uri = crate::TOKIO_RT.block_on(spawn_delayed_ws_server(Duration::from_secs(30)));

    #[cfg(feature = "blocking")]
    {
        let start = Instant::now();
        let client = nyquest::ClientBuilder::default()
            .request_timeout(Duration::from_secs(1))
            .build_blocking()
            .unwrap();
        let res = client.connect_websocket(slow_uri.clone(), None);
        assert!(res.is_err(), "connect_websocket should time out");
        assert!(start.elapsed() < Duration::from_secs(10));
    }

    #[cfg(feature = "async")]
    crate::TOKIO_RT.block_on(async {
        let start = Instant::now();
        let client = nyquest::ClientBuilder::default()
            .request_timeout(Duration::from_secs(1))
            .build_async()
            .await
            .unwrap();
        let connecting = tokio::time::timeout(
            Duration::from_secs(10),
            client.connect_websocket(slow_uri.clone(), None),
        );
        let res = connecting
            .await
            .expect("connect_websocket should not hang longer than 10 seconds");
        assert!(res.is_err(), "connect_websocket should time out");
        assert!(start.elapsed() < Duration::from_secs(10));
    });
}

#[test]
fn websocket_client_options_request_didnt_timeout() {
    init_ws_backend();
    let delayed_uri = crate::TOKIO_RT.block_on(spawn_delayed_ws_server(Duration::from_secs(3)));

    #[cfg(feature = "blocking")]
    {
        let client = nyquest::ClientBuilder::default()
            .request_timeout(Duration::from_secs(10))
            .build_blocking()
            .unwrap();
        let mut ws = client.connect_websocket(delayed_uri.clone(), None).unwrap();
        ws.sender
            .send(WebSocketMessage::Utf8("didnt-timeout".into()))
            .unwrap();
        assert_eq!(
            ws.receiver.receive().unwrap(),
            WebSocketMessage::Utf8("didnt-timeout".into())
        );
        ws.sender.close().unwrap();
    }

    #[cfg(feature = "async")]
    crate::TOKIO_RT.block_on(async {
        let client = nyquest::ClientBuilder::default()
            .request_timeout(Duration::from_secs(10))
            .build_async()
            .await
            .unwrap();
        let mut ws = client.connect_websocket(delayed_uri, None).await.unwrap();
        ws.sender
            .send(WebSocketMessage::Utf8("didnt-timeout".into()))
            .await
            .unwrap();
        assert_eq!(
            ws.receiver.next().await.unwrap().unwrap(),
            WebSocketMessage::Utf8("didnt-timeout".into())
        );
        ws.sender.close().await.unwrap();
    });
}

#[test]
fn websocket_client_options_late_sendrecv_didnt_timeout() {
    init_ws_backend();
    let delayed_uri = crate::TOKIO_RT.block_on(spawn_delayed_ws_server(Duration::from_secs(0)));

    #[cfg(feature = "blocking")]
    {
        let client = nyquest::ClientBuilder::default()
            .request_timeout(Duration::from_secs(1))
            .build_blocking()
            .unwrap();
        let mut ws = client.connect_websocket(delayed_uri.clone(), None).unwrap();
        std::thread::sleep(Duration::from_millis(1500));
        ws.sender
            .send(WebSocketMessage::Utf8("didnt-timeout".into()))
            .unwrap();
        assert_eq!(
            ws.receiver.receive().unwrap(),
            WebSocketMessage::Utf8("didnt-timeout".into())
        );
        ws.sender.close().unwrap();
    }

    #[cfg(feature = "async")]
    crate::TOKIO_RT.block_on(async {
        let client = nyquest::ClientBuilder::default()
            .request_timeout(Duration::from_secs(1))
            .build_async()
            .await
            .unwrap();
        let mut ws = client.connect_websocket(delayed_uri, None).await.unwrap();
        tokio::time::sleep(Duration::from_millis(1500)).await;
        ws.sender
            .send(WebSocketMessage::Utf8("didnt-timeout".into()))
            .await
            .unwrap();
        assert_eq!(
            ws.receiver.next().await.unwrap().unwrap(),
            WebSocketMessage::Utf8("didnt-timeout".into())
        );
        ws.sender.close().await.unwrap();
    });
}
