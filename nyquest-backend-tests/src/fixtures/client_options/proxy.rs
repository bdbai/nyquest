#[cfg(test)]
mod tests {
    use std::{convert::Infallible, sync::Arc};

    use http_body_util::Full;
    use hyper::{ext::ReasonPhrase, service::service_fn};
    use nyquest::{client::CustomProxy, Request as NyquestRequest};

    use crate::{hyper_fixture_collection::HyperFixtureHandle, *};

    fn gen_response_proxied() -> Response<Full<Bytes>> {
        Response::new(Full::new(Bytes::from("proxied")))
    }
    fn gen_response_direct() -> Response<Full<Bytes>> {
        Response::new(Full::new(Bytes::from("direct")))
    }

    async fn proxy_handler(req: Request<body::Incoming>) -> FixtureAssertionResult {
        let is_proxied = req.uri().host().is_some();

        let res = if is_proxied {
            gen_response_proxied()
        } else {
            gen_response_direct()
        };
        (res.into(), Ok(()))
    }

    async fn connect_handler(req: Request<body::Incoming>) -> FixtureAssertionResult {
        if req.method() == hyper::Method::CONNECT {
            // libcurl uses a CONNECT request tunnel even for HTTP requests
            tokio::spawn(async move {
                let upgraded = hyper::upgrade::on(req).await.unwrap();
                hyper::server::conn::http1::Builder::new()
                    .serve_connection(
                        upgraded,
                        service_fn(|_| async { Ok::<_, Infallible>(gen_response_proxied()) }),
                    )
                    .await
                    .ok();
            });
            let mut res = Response::new(Full::default());
            res.extensions_mut()
                .insert(ReasonPhrase::from_static(b"Connection Established"));
            (res.into(), Ok(()))
        } else {
            let res = Response::builder()
                .status(400)
                .body(Full::default())
                .unwrap();
            (res.into(), Ok(()))
        }
    }

    #[must_use = "the fixture handles must be kept alive for the duration of the test"]
    struct ProxyFixtureSetup {
        proxy_url: String,
        _main_handle: HyperFixtureHandle<&'static HyperFixtureCollection>,
        _proxy_handle: HyperFixtureHandle<Arc<HyperFixtureCollection>>,
        _proxy_connect_handle: HyperFixtureHandle<Arc<HyperFixtureCollection>>,
    }

    fn setup_proxy_fixture(path: &str) -> ProxyFixtureSetup {
        let proxy_hyper_collection = Arc::new(HyperFixtureCollection::new());
        let proxy_port = TOKIO_RT
            .block_on({
                let proxy_hyper_collection = proxy_hyper_collection.clone();
                hyper_fixture_collection::spawn_service(proxy_hyper_collection)
            })
            .unwrap();
        let _main_handle = crate::add_hyper_fixture(path, proxy_handler);
        let _proxy_handle = hyper_fixture_collection::add_hyper_fixture(
            proxy_hyper_collection.clone(),
            path,
            proxy_handler,
        );
        let _proxy_connect_handle = hyper_fixture_collection::add_hyper_fixture(
            proxy_hyper_collection,
            "",
            connect_handler,
        );

        ProxyFixtureSetup {
            proxy_url: format!("http://127.0.0.1:{proxy_port}"),
            _main_handle,
            _proxy_handle,
            _proxy_connect_handle,
        }
    }

    #[test]
    #[cfg(not(feature = "winrt"))] // WinRT HttpClient does not support custom proxies
    fn test_custom_http_proxy() {
        const PATH: &str = "client_options/custom_http_proxy";
        let proxy_fixture_setup = setup_proxy_fixture(PATH);

        let assertions = |status: u16, body: String| {
            assert_eq!(status, 200);
            assert_eq!(body, "proxied");
        };

        let custom_proxy = CustomProxy::http(proxy_fixture_setup.proxy_url);

        #[cfg(feature = "blocking")]
        {
            let builder = crate::init_builder_blocking().unwrap();
            let client = builder
                .custom_proxy(custom_proxy.clone())
                .build_blocking()
                .unwrap();
            let res = client.request(NyquestRequest::get(PATH)).unwrap();
            let status = res.status().into();
            let body = res.text().unwrap();
            assertions(status, body);
        }

        #[cfg(feature = "async")]
        {
            let (status, body) = TOKIO_RT.block_on(async {
                let builder = crate::init_builder().await.unwrap();
                let client = builder
                    .custom_proxy(custom_proxy)
                    .build_async()
                    .await
                    .unwrap();
                let res = client.request(NyquestRequest::get(PATH)).await.unwrap();
                let status = res.status().into();
                let body = res.text().await.unwrap();
                (status, body)
            });

            assertions(status, body);
        }
    }

    // TODO: test_custom_all_proxy_for_https, test_custom_https_proxy_for_https
    #[test]
    #[cfg(not(feature = "winrt"))] // WinRT HttpClient does not support custom proxies
    fn test_custom_all_proxy_for_http() {
        const PATH: &str = "client_options/custom_all_proxy_for_http";
        let proxy_fixture_setup = setup_proxy_fixture(PATH);

        let assertions = |status: u16, body: String| {
            assert_eq!(status, 200);
            assert_eq!(body, "proxied");
        };

        let custom_proxy = CustomProxy::http(proxy_fixture_setup.proxy_url.clone())
            .with_https(proxy_fixture_setup.proxy_url);

        #[cfg(feature = "blocking")]
        {
            let builder = crate::init_builder_blocking().unwrap();
            let client = builder
                .custom_proxy(custom_proxy.clone())
                .build_blocking()
                .unwrap();
            let res = client.request(NyquestRequest::get(PATH)).unwrap();
            let status = res.status().into();
            let body = res.text().unwrap();
            assertions(status, body);
        }

        #[cfg(feature = "async")]
        {
            let (status, body) = TOKIO_RT.block_on(async {
                let builder = crate::init_builder().await.unwrap();
                let client = builder
                    .custom_proxy(custom_proxy)
                    .build_async()
                    .await
                    .unwrap();
                let res = client.request(NyquestRequest::get(PATH)).await.unwrap();
                let status = res.status().into();
                let body = res.text().await.unwrap();
                (status, body)
            });

            assertions(status, body);
        }
    }

    #[test]
    fn test_custom_https_proxy_for_http() {
        const PATH: &str = "client_options/custom_https_proxy_for_http";
        let proxy_fixture_setup = setup_proxy_fixture(PATH);

        let assertions = |status: u16, body: String| {
            assert_eq!(status, 200);
            assert_eq!(body, "direct");
        };

        let custom_proxy = CustomProxy::https(proxy_fixture_setup.proxy_url);

        #[cfg(feature = "blocking")]
        {
            let builder = crate::init_builder_blocking().unwrap();
            let client = builder
                .custom_proxy(custom_proxy.clone())
                .build_blocking()
                .unwrap();
            let res = client.request(NyquestRequest::get(PATH)).unwrap();
            let status = res.status().into();
            let body = res.text().unwrap();
            assertions(status, body);
        }

        #[cfg(feature = "async")]
        {
            let (status, body) = TOKIO_RT.block_on(async {
                let builder = crate::init_builder().await.unwrap();
                let client = builder
                    .custom_proxy(custom_proxy)
                    .build_async()
                    .await
                    .unwrap();
                let res = client.request(NyquestRequest::get(PATH)).await.unwrap();
                let status = res.status().into();
                let body = res.text().await.unwrap();
                (status, body)
            });

            assertions(status, body);
        }
    }

    #[test]
    fn test_custom_proxy_bypass() {
        const PATH: &str = "client_options/custom_proxy_bypass";
        let proxy_fixture_setup = setup_proxy_fixture(PATH);

        let assertions = |status: u16, body: String| {
            assert_eq!(status, 200);
            assert_eq!(body, "direct");
        };

        let custom_proxy = CustomProxy::http(proxy_fixture_setup.proxy_url.clone())
            .with_https(proxy_fixture_setup.proxy_url)
            .with_bypass("localhost.");

        #[cfg(feature = "blocking")]
        {
            let builder = crate::init_builder_blocking().unwrap();
            let client = builder
                .custom_proxy(custom_proxy.clone())
                .build_blocking()
                .unwrap();
            let res = client.request(NyquestRequest::get(PATH)).unwrap();
            let status = res.status().into();
            let body = res.text().unwrap();
            assertions(status, body);
        }

        #[cfg(feature = "async")]
        {
            let (status, body) = TOKIO_RT.block_on(async {
                let builder = crate::init_builder().await.unwrap();
                let client = builder
                    .custom_proxy(custom_proxy)
                    .build_async()
                    .await
                    .unwrap();
                let res = client.request(NyquestRequest::get(PATH)).await.unwrap();
                let status = res.status().into();
                let body = res.text().await.unwrap();
                (status, body)
            });

            assertions(status, body);
        }
    }

    #[test]
    fn test_no_proxy() {
        const PATH: &str = "client_options/no_proxy";
        let _proxy_fixture_setup = setup_proxy_fixture(PATH);

        let assertions = |status: u16, body: String| {
            assert_eq!(status, 200);
            assert_eq!(body, "direct");
        };

        #[cfg(feature = "blocking")]
        {
            let builder = crate::init_builder_blocking().unwrap();
            let client = builder.no_proxy().build_blocking().unwrap();
            let res = client.request(NyquestRequest::get(PATH)).unwrap();
            let status = res.status().into();
            let body = res.text().unwrap();
            assertions(status, body);
        }

        #[cfg(feature = "async")]
        {
            let (status, body) = TOKIO_RT.block_on(async {
                let builder = crate::init_builder().await.unwrap();
                let client = builder.no_proxy().build_async().await.unwrap();
                let res = client.request(NyquestRequest::get(PATH)).await.unwrap();
                let status = res.status().into();
                let body = res.text().await.unwrap();
                (status, body)
            });

            assertions(status, body);
        }
    }
}
