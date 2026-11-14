use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};

use futures_util::{SinkExt, StreamExt};
use nyquest::WebSocketMessage;
use tokio::net::TcpListener;
use tokio_rustls::{
    rustls::{
        self,
        pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer},
    },
    TlsAcceptor,
};
use tokio_tungstenite::tungstenite::protocol::Message;

use crate::websocket::init_ws_backend;

async fn spawn_tls_ws_server() -> (String, Arc<AtomicUsize>) {
    let cert = CertificateDer::from(
        include_bytes!("../../fixtures/client_options/certificate_test_data/expired-cert.der")
            .to_vec(),
    );
    let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(
        include_bytes!("../../fixtures/client_options/certificate_test_data/expired-key.der")
            .to_vec(),
    ));
    let config = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .unwrap()
    .with_no_client_auth()
    .with_single_cert(vec![cert], key)
    .unwrap();
    let acceptor = TlsAcceptor::from(Arc::new(config));
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let connections = Arc::new(AtomicUsize::new(0));
    let accepted = connections.clone();

    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                break;
            };
            accepted.fetch_add(1, Ordering::SeqCst);
            let acceptor = acceptor.clone();
            tokio::spawn(async move {
                let Ok(tls_stream) = acceptor.accept(stream).await else {
                    return;
                };
                let Ok(mut websocket) = tokio_tungstenite::accept_async(tls_stream).await else {
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

    (format!("wss://127.0.0.1:{port}/tls"), connections)
}

#[test]
fn websocket_client_options_certificate_expired() {
    init_ws_backend();
    let (wss_url, connections) = crate::TOKIO_RT.block_on(spawn_tls_ws_server());

    #[cfg(feature = "blocking")]
    {
        let client = nyquest::ClientBuilder::default()
            .no_proxy()
            .no_caching()
            .request_timeout(Duration::from_secs(10))
            .build_blocking()
            .unwrap();
        let before = connections.load(Ordering::SeqCst);
        let res = client.connect_websocket(wss_url.clone(), None);
        assert!(res.is_err(), "expired certificate should be rejected");
        assert!(
            connections.load(Ordering::SeqCst) > before,
            "websocket handshake did not reach the TLS server"
        );
    }

    #[cfg(feature = "async")]
    crate::TOKIO_RT.block_on(async {
        let client = nyquest::ClientBuilder::default()
            .no_proxy()
            .no_caching()
            .request_timeout(Duration::from_secs(10))
            .build_async()
            .await
            .unwrap();
        let before = connections.load(Ordering::SeqCst);
        let res = client.connect_websocket(wss_url, None).await;
        assert!(res.is_err(), "expired certificate should be rejected");
        assert!(
            connections.load(Ordering::SeqCst) > before,
            "websocket handshake did not reach the TLS server"
        );
    });
}

#[test]
fn websocket_client_options_certificate_expired_ignored() {
    init_ws_backend();
    let (wss_url, _connections) = crate::TOKIO_RT.block_on(spawn_tls_ws_server());

    #[cfg(feature = "blocking")]
    {
        let client = nyquest::ClientBuilder::default()
            .no_proxy()
            .no_caching()
            .request_timeout(Duration::from_secs(10))
            .dangerously_ignore_certificate_errors()
            .build_blocking()
            .unwrap();
        let mut ws = client.connect_websocket(wss_url.clone(), None).unwrap();
        ws.sender
            .send(WebSocketMessage::Utf8("tls-ok".into()))
            .unwrap();
        assert_eq!(
            ws.receiver.receive().unwrap(),
            WebSocketMessage::Utf8("tls-ok".into())
        );
        ws.sender.close().unwrap();
    }

    #[cfg(feature = "async")]
    crate::TOKIO_RT.block_on(async {
        let client = nyquest::ClientBuilder::default()
            .no_proxy()
            .no_caching()
            .request_timeout(Duration::from_secs(10))
            .dangerously_ignore_certificate_errors()
            .build_async()
            .await
            .unwrap();
        let mut ws = client.connect_websocket(wss_url, None).await.unwrap();
        ws.sender
            .send(WebSocketMessage::Utf8("tls-ok".into()))
            .await
            .unwrap();
        assert_eq!(
            ws.receiver.receive().await.unwrap(),
            WebSocketMessage::Utf8("tls-ok".into())
        );
        ws.sender.close().await.unwrap();
    });
}
