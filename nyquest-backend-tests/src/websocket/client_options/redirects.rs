#[cfg(not(any(feature = "winrt", feature = "nsurlsession", feature = "curl")))]
// winrt MessageWebSocket, nsurlsession WebSocketTaskDelegate, and curl CONNECT_ONLY websocket mode do not support follow_redirects control
#[test]
fn websocket_client_options_redirects() {
    use http_body_util::Full;
    use hyper::{body::Bytes, Response};
    use nyquest::WebSocketMessage;

    use crate::websocket::{init_ws_backend, spawn_server};

    const PATH: &str = "client_options/ws_redirect";

    init_ws_backend();
    let target_ws_uri = crate::TOKIO_RT.block_on(spawn_server());
    let target_http_uri = target_ws_uri.replace("ws://", "http://");
    let _handle = crate::add_hyper_fixture(PATH, move |_req| {
        let loc = format!("{target_http_uri}/redirected");
        async move {
            let res = Response::builder()
                .status(302)
                .header("Location", loc)
                .body(Full::new(Bytes::from_static(b"redirecting")))
                .unwrap();
            (res, Ok(()))
        }
    });

    #[cfg(feature = "blocking")]
    {
        let redirect_ws_url = crate::TOKIO_RT.block_on(async {
            let port = crate::init_main_service_port().await.unwrap();
            format!("ws://127.0.0.1:{port}/{PATH}")
        });

        // Default: follow_redirects = true
        let client = nyquest::ClientBuilder::default().build_blocking().unwrap();
        let mut ws = client
            .connect_websocket(redirect_ws_url.clone(), None)
            .unwrap();
        ws.sender
            .send(WebSocketMessage::Utf8("after-redirect".into()))
            .unwrap();
        assert_eq!(
            ws.receiver.receive().unwrap(),
            WebSocketMessage::Utf8("after-redirect".into())
        );
        ws.sender.close().unwrap();

        // no_redirects
        let client_no_redir = nyquest::ClientBuilder::default()
            .no_redirects()
            .build_blocking()
            .unwrap();
        assert!(
            client_no_redir
                .connect_websocket(redirect_ws_url, None)
                .is_err(),
            "connect_websocket should fail on 302 when no_redirects is set"
        );
    }

    #[cfg(feature = "async")]
    crate::TOKIO_RT.block_on(async {
        use futures_util::{SinkExt, StreamExt};

        let port = crate::init_main_service_port().await.unwrap();
        let redirect_ws_url = format!("ws://127.0.0.1:{port}/{PATH}");

        // Default: follow_redirects = true
        let client = nyquest::ClientBuilder::default()
            .build_async()
            .await
            .unwrap();
        let mut ws = client
            .connect_websocket(redirect_ws_url.clone(), None)
            .await
            .unwrap();
        ws.sender
            .send(WebSocketMessage::Utf8("after-redirect".into()))
            .await
            .unwrap();
        assert_eq!(
            ws.receiver.next().await.unwrap().unwrap(),
            WebSocketMessage::Utf8("after-redirect".into())
        );
        ws.sender.close().await.unwrap();

        // no_redirects
        let client_no_redir = nyquest::ClientBuilder::default()
            .no_redirects()
            .build_async()
            .await
            .unwrap();
        assert!(
            client_no_redir
                .connect_websocket(redirect_ws_url, None)
                .await
                .is_err(),
            "connect_websocket should fail on 302 when no_redirects is set"
        );
    });
}
