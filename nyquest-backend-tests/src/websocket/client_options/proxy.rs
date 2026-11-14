use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

use nyquest::{client::CustomProxy, WebSocketMessage};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use crate::websocket::{init_ws_backend, spawn_server};

async fn spawn_ws_proxy_server(target_ws_uri: &str) -> (String, Arc<AtomicUsize>) {
    let target_addr = target_ws_uri
        .strip_prefix("ws://")
        .unwrap()
        .split('/')
        .next()
        .unwrap()
        .to_owned();
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let proxy_port = listener.local_addr().unwrap().port();
    let proxy_hits = Arc::new(AtomicUsize::new(0));
    let hits = proxy_hits.clone();

    tokio::spawn(async move {
        loop {
            let Ok((mut client_stream, _)) = listener.accept().await else {
                break;
            };
            hits.fetch_add(1, Ordering::SeqCst);
            let target_addr = target_addr.clone();
            tokio::spawn(async move {
                let mut peek_buf = [0u8; 8];
                let Ok(n) = client_stream.peek(&mut peek_buf).await else {
                    return;
                };
                let mut upstream = tokio::net::TcpStream::connect(&target_addr).await.unwrap();
                if n >= 8 && peek_buf.starts_with(b"CONNECT ") {
                    let mut req_buf = Vec::new();
                    let mut byte = [0u8; 1];
                    while !req_buf.ends_with(b"\r\n\r\n") {
                        if client_stream.read_exact(&mut byte).await.is_err() {
                            return;
                        }
                        req_buf.push(byte[0]);
                    }
                    let _ = client_stream
                        .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
                        .await;
                }
                let _ = tokio::io::copy_bidirectional(&mut client_stream, &mut upstream).await;
            });
        }
    });

    (format!("http://127.0.0.1:{proxy_port}"), proxy_hits)
}

#[cfg(not(feature = "winrt"))] // WinRT does not support custom proxies
#[test]
fn websocket_client_options_custom_proxy() {
    init_ws_backend();
    let ws_uri = crate::TOKIO_RT.block_on(spawn_server());
    let (proxy_url, proxy_hits) = crate::TOKIO_RT.block_on(spawn_ws_proxy_server(&ws_uri));
    // Use localhost. so WinHTTP does not bypass the proxy for 127.0.0.1 loopback,
    // and set an explicit dummy bypass host so libcurl overrides its built-in "localhost" noproxy default.
    let target_uri = format!("{}/proxied", ws_uri.replace("127.0.0.1", "localhost."));
    let custom_proxy = CustomProxy::http(proxy_url.clone())
        .with_https(proxy_url)
        .with_bypass("example.invalid");

    #[cfg(feature = "blocking")]
    {
        let before = proxy_hits.load(Ordering::SeqCst);
        let client = nyquest::ClientBuilder::default()
            .custom_proxy(custom_proxy.clone())
            .build_blocking()
            .unwrap();
        let mut ws = client.connect_websocket(target_uri.clone(), None).unwrap();
        ws.sender
            .send(WebSocketMessage::Utf8("via-proxy".into()))
            .unwrap();
        assert_eq!(
            ws.receiver.receive().unwrap(),
            WebSocketMessage::Utf8("via-proxy".into())
        );
        assert!(
            proxy_hits.load(Ordering::SeqCst) > before,
            "websocket connection did not go through custom proxy"
        );
        ws.sender.close().unwrap();
    }

    #[cfg(feature = "async")]
    crate::TOKIO_RT.block_on(async {
        let before = proxy_hits.load(Ordering::SeqCst);
        let client = nyquest::ClientBuilder::default()
            .custom_proxy(custom_proxy)
            .build_async()
            .await
            .unwrap();
        let mut ws = client.connect_websocket(target_uri, None).await.unwrap();
        ws.sender
            .send(WebSocketMessage::Utf8("via-proxy".into()))
            .await
            .unwrap();
        assert_eq!(
            ws.receiver.receive().await.unwrap(),
            WebSocketMessage::Utf8("via-proxy".into())
        );
        assert!(
            proxy_hits.load(Ordering::SeqCst) > before,
            "websocket connection did not go through custom proxy"
        );
        ws.sender.close().await.unwrap();
    });
}

#[test]
fn websocket_client_options_custom_proxy_bypass() {
    init_ws_backend();
    let ws_uri = crate::TOKIO_RT.block_on(spawn_server());
    let (proxy_url, proxy_hits) = crate::TOKIO_RT.block_on(spawn_ws_proxy_server(&ws_uri));
    let target_uri = format!("{}/bypass", ws_uri.replace("127.0.0.1", "localhost."));
    let custom_proxy = CustomProxy::http(proxy_url.clone())
        .with_https(proxy_url)
        .with_bypass("localhost.");

    #[cfg(feature = "blocking")]
    {
        let client = nyquest::ClientBuilder::default()
            .custom_proxy(custom_proxy.clone())
            .build_blocking()
            .unwrap();
        let mut ws = client.connect_websocket(target_uri.clone(), None).unwrap();
        ws.sender
            .send(WebSocketMessage::Utf8("bypass-proxy".into()))
            .unwrap();
        assert_eq!(
            ws.receiver.receive().unwrap(),
            WebSocketMessage::Utf8("bypass-proxy".into())
        );
        assert_eq!(proxy_hits.load(Ordering::SeqCst), 0);
        ws.sender.close().unwrap();
    }

    #[cfg(feature = "async")]
    crate::TOKIO_RT.block_on(async {
        let client = nyquest::ClientBuilder::default()
            .custom_proxy(custom_proxy)
            .build_async()
            .await
            .unwrap();
        let mut ws = client.connect_websocket(target_uri, None).await.unwrap();
        ws.sender
            .send(WebSocketMessage::Utf8("bypass-proxy".into()))
            .await
            .unwrap();
        assert_eq!(
            ws.receiver.receive().await.unwrap(),
            WebSocketMessage::Utf8("bypass-proxy".into())
        );
        assert_eq!(proxy_hits.load(Ordering::SeqCst), 0);
        ws.sender.close().await.unwrap();
    });
}
