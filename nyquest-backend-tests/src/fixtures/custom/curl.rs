use http_body_util::Full;
use hyper::{body::Bytes, Response};
use nyquest_backend_curl::{curl_sys, CurlBackend, CurlHandle, HandleCustomizer};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

fn register_fixture(path: &'static str) -> impl Drop {
    crate::add_hyper_fixture(path, |req| async move {
        let referer = req
            .headers()
            .get("referer")
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_owned();
        (Response::new(Full::new(Bytes::from(referer))), Ok(()))
    })
}

#[cfg(feature = "blocking")]
mod blocking_tests {
    use std::time::Duration;

    use nyquest::Request as NyquestRequest;

    use super::*;

    #[test]
    fn customizer_runs_on_every_request_of_a_reused_handle() {
        struct CountingCustomizer {
            calls: Arc<AtomicUsize>,
        }
        impl HandleCustomizer for CountingCustomizer {
            fn customize_handle(
                &self,
                mut handle: CurlHandle<'_>,
            ) -> Result<(), nyquest_backend_curl::CurlCodeContext> {
                self.calls.fetch_add(1, Ordering::SeqCst);
                unsafe {
                    handle.setopt_ptr_raw(curl_sys::CURLOPT_REFERER, c"reused-handle".as_ptr())?;
                }
                Ok(())
            }
        }

        const PATH: &str = "/custom/curl/reused-handle";
        let _handle = register_fixture(PATH);
        let calls = Arc::new(AtomicUsize::new(0));
        let backend = CurlBackend::customize(CountingCustomizer {
            calls: Arc::clone(&calls),
        });
        let client = crate::init_builder_from_backend_blocking(&backend)
            .unwrap()
            .build_blocking()
            .unwrap();

        for _ in 0..3 {
            let req = NyquestRequest::get(PATH);
            let res = client.request(req).unwrap();
            assert_eq!(res.status(), 200);
            assert_eq!(res.text().unwrap(), "reused-handle");
        }
        assert_eq!(calls.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn customizer_applies_after_client_options() {
        struct CustomOpts;
        impl HandleCustomizer for CustomOpts {
            fn customize_handle(
                &self,
                mut handle: CurlHandle<'_>,
            ) -> Result<(), nyquest_backend_curl::CurlCodeContext> {
                unsafe {
                    handle
                        .setopt_ptr_raw(curl_sys::CURLOPT_REFERER, c"custom-referrer".as_ptr())?;
                }
                Ok(())
            }
        }

        const PATH: &str = "/custom/curl/override";
        let _handle = register_fixture(PATH);
        let backend = CurlBackend::customize(CustomOpts);
        let client = crate::init_builder_from_backend_blocking(&backend)
            .unwrap()
            .request_timeout(Duration::from_secs(30))
            .build_blocking()
            .unwrap();

        let req = NyquestRequest::get(PATH);
        let res = client.request(req).unwrap();
        assert_eq!(res.status(), 200);
        assert_eq!(res.text().unwrap(), "custom-referrer");
    }
}

#[cfg(feature = "async")]
mod async_tests {
    use nyquest::r#async::Request as NyquestRequest;

    use super::*;

    #[tokio::test]
    async fn client_applies_custom_curl_opts() {
        struct AsyncCustomAgent;
        impl HandleCustomizer for AsyncCustomAgent {
            fn customize_handle(
                &self,
                mut handle: CurlHandle<'_>,
            ) -> Result<(), nyquest_backend_curl::CurlCodeContext> {
                unsafe {
                    handle.setopt_ptr_raw(
                        curl_sys::CURLOPT_REFERER,
                        c"async-custom-referrer".as_ptr(),
                    )?;
                }
                Ok(())
            }
        }

        const PATH: &str = "/custom/curl/async";
        let _handle = register_fixture(PATH);
        let backend = CurlBackend::customize(AsyncCustomAgent);
        let client = crate::init_builder_from_backend(&backend)
            .await
            .unwrap()
            .build_async()
            .await
            .unwrap();

        let req = NyquestRequest::get(PATH);
        let res = client.request(req).await.unwrap();
        assert_eq!(res.status(), 200);
        assert_eq!(res.text().await.unwrap(), "async-custom-referrer");
    }
}
