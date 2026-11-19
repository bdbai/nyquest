#[cfg(test)]
mod tests {
    use std::{
        convert::Infallible,
        sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        },
        time::Duration,
    };

    use http_body_util::Full;
    use hyper::{body::Bytes, server::conn::http1, service::service_fn, Response};
    use hyper_util::rt::TokioIo;
    use nyquest::{ClientBuilder, Request as NyquestRequest};
    use tokio::net::TcpListener;
    use tokio_rustls::{
        rustls::{
            self,
            pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer},
        },
        TlsAcceptor,
    };

    use crate::{init_backend_once, TOKIO_RT};

    const TIMEOUT: Duration = Duration::from_secs(10);
    const BODY: &str = "local expired certificate fixture";

    struct ExpiredCertificateServer {
        url: String,
        connections: Arc<AtomicUsize>,
        task: tokio::task::JoinHandle<()>,
    }

    impl Drop for ExpiredCertificateServer {
        fn drop(&mut self) {
            self.task.abort();
        }
    }

    impl ExpiredCertificateServer {
        async fn start() -> Self {
            // Test-only self-signed certificate, valid for 127.0.0.1 from 2020 to 2021.
            // It is also untrusted, since the client API cannot install a test CA.
            let cert = CertificateDer::from(
                include_bytes!("certificate_test_data/expired-cert.der").to_vec(),
            );
            let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(
                include_bytes!("certificate_test_data/expired-key.der").to_vec(),
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
            let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
                .await
                .unwrap();
            let url = format!("https://{}/", listener.local_addr().unwrap());
            let connections = Arc::new(AtomicUsize::new(0));
            let accepted = connections.clone();
            let task = tokio::spawn(async move {
                loop {
                    let (stream, _) = listener.accept().await.expect("TLS listener failed");
                    accepted.fetch_add(1, Ordering::SeqCst);
                    let acceptor = acceptor.clone();
                    tokio::spawn(async move {
                        if let Ok(stream) = acceptor.accept(stream).await {
                            let _ = http1::Builder::new()
                                .serve_connection(
                                    TokioIo::new(stream),
                                    service_fn(|_| async {
                                        Ok::<_, Infallible>(Response::new(Full::new(
                                            Bytes::from_static(BODY.as_bytes()),
                                        )))
                                    }),
                                )
                                .await;
                        }
                    });
                }
            });
            Self {
                url,
                connections,
                task,
            }
        }
    }

    fn builder() -> ClientBuilder {
        init_backend_once();
        ClientBuilder::default()
            .no_proxy()
            .no_caching()
            .request_timeout(TIMEOUT)
    }

    #[test]
    fn test_certificate_expired() {
        let server = TOKIO_RT.block_on(ExpiredCertificateServer::start());

        #[cfg(feature = "blocking")]
        {
            let client = builder().build_blocking().unwrap();
            let before = server.connections.load(Ordering::SeqCst);
            let err = client
                .request(NyquestRequest::get(server.url.clone()))
                .unwrap_err();
            assert!(!matches!(err, nyquest::Error::RequestTimeout), "{err:?}");
            assert!(
                server.connections.load(Ordering::SeqCst) > before,
                "request did not reach the TLS server"
            );
        }

        #[cfg(feature = "async")]
        TOKIO_RT.block_on(async {
            let client = builder().build_async().await.unwrap();
            let before = server.connections.load(Ordering::SeqCst);
            let err = client
                .request(NyquestRequest::get(server.url.clone()))
                .await
                .unwrap_err();
            assert!(!matches!(err, nyquest::Error::RequestTimeout), "{err:?}");
            assert!(
                server.connections.load(Ordering::SeqCst) > before,
                "request did not reach the TLS server"
            );
        });
    }

    #[test]
    fn test_certificate_expired_ignored() {
        let server = TOKIO_RT.block_on(ExpiredCertificateServer::start());

        #[cfg(feature = "blocking")]
        {
            let client = builder()
                .dangerously_ignore_certificate_errors()
                .build_blocking()
                .unwrap();
            let response = client
                .request(NyquestRequest::get(server.url.clone()))
                .unwrap()
                .with_successful_status()
                .unwrap();
            assert_eq!(response.text().unwrap(), BODY);
        }

        #[cfg(feature = "async")]
        TOKIO_RT.block_on(async {
            let client = builder()
                .dangerously_ignore_certificate_errors()
                .build_async()
                .await
                .unwrap();
            let response = client
                .request(NyquestRequest::get(server.url.clone()))
                .await
                .unwrap()
                .with_successful_status()
                .unwrap();
            assert_eq!(response.text().await.unwrap(), BODY);
        });
    }
}
