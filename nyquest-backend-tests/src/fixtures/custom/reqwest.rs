use http_body_util::Full;
use hyper::{body::Bytes, Response};
use nyquest_backend_reqwest::{reqwest, ReqwestBackend};

fn register_fixture(path: &'static str) -> impl Drop {
    crate::add_hyper_fixture(path, |req| async move {
        let body = req
            .headers()
            .get("user-agent")
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_owned();
        (Response::new(Full::new(Bytes::from(body))), Ok(()))
    })
}

#[cfg(feature = "blocking")]
mod blocking_tests {
    use nyquest::blocking::Request as NyquestRequest;

    use super::*;

    #[test]
    fn blocking_client_applies_builder_customization() {
        const PATH: &str = "/custom/reqwest/blocking";
        let _handle = register_fixture(PATH);
        let customize = |builder: reqwest::ClientBuilder| {
            builder
                .user_agent("customized-agent")
                .pool_max_idle_per_host(0)
        };
        let backend = ReqwestBackend::customize(customize);
        let client = crate::init_builder_from_backend_blocking(&backend)
            .unwrap()
            .build_blocking()
            .unwrap();

        let req = NyquestRequest::get(PATH);
        let res = client.request(req).unwrap();
        assert_eq!(res.status(), 200);
        assert_eq!(res.text().unwrap(), "customized-agent");
    }

    #[cfg(feature = "blocking")]
    #[test]
    fn invalid_customization_is_reported_when_building_the_client() {
        let customize = |builder: reqwest::ClientBuilder| {
            builder.min_tls_version(reqwest::tls::Version::TLS_1_3)
        };
        let backend = ReqwestBackend::customize(customize);
        crate::init_builder_from_backend_blocking(&backend)
            .expect_err("the customized client builder should not build");
    }
}

#[cfg(feature = "async")]
mod async_tests {
    use nyquest::r#async::Request as NyquestRequest;

    use super::*;

    #[tokio::test]
    async fn async_client_applies_builder_customization() {
        const PATH: &str = "/custom/reqwest/async";
        let _handle = register_fixture(PATH);
        let customize =
            |builder: reqwest::ClientBuilder| builder.user_agent("async-customized-agent");
        let backend = ReqwestBackend::customize(customize);
        let client = crate::init_builder_from_backend(&backend)
            .await
            .unwrap()
            .build_async()
            .await
            .unwrap();

        let req = NyquestRequest::get(PATH);
        let res = client.request(req).await.unwrap();
        assert_eq!(res.status(), 200);
        assert_eq!(res.text().await.unwrap(), "async-customized-agent");
    }
}
