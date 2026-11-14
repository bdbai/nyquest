use std::time::{Duration, Instant};

use tokio::net::TcpListener;

use crate::websocket::init_ws_backend;

#[test]
fn websocket_client_options_request_timeout() {
    init_ws_backend();
    let slow_uri = crate::TOKIO_RT.block_on(async {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            loop {
                let Ok((_stream, _)) = listener.accept().await else {
                    break;
                };
                tokio::spawn(async move {
                    tokio::time::sleep(Duration::from_secs(30)).await;
                });
            }
        });
        format!("ws://127.0.0.1:{port}/slow")
    });

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
        let res = client.connect_websocket(slow_uri, None).await;
        assert!(res.is_err(), "connect_websocket should time out");
        assert!(start.elapsed() < Duration::from_secs(10));
    });
}
