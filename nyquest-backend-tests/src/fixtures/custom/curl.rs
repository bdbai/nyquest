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

    const MOCK_CA_BUNDLE: &[u8] = b"-----BEGIN CERTIFICATE-----
MIIDBTCCAe2gAwIBAgIUXz1rwjtEHA774mvQZFx61iiAH60wDQYJKoZIhvcNAQEL
BQAwEjEQMA4GA1UEAwwHTW9jayBDQTAeFw0yNjEwMTgxMTEyNTlaFw0yNjEwMTkx
MTEyNTlaMBIxEDAOBgNVBAMMB01vY2sgQ0EwggEiMA0GCSqGSIb3DQEBAQUAA4IB
DwAwggEKAoIBAQCFm8PkY5xreLIvF25tQsr/vNRdIO2ipCZcZoT7HsTzTzHKAwaF
dndCtz/JuQlm16gymbAIcUpGW6nd0LHVdSsMu0Qw/DSHFfvhtqgOW8rNsqACUxHS
xpOq4JRhrLPj2kKyEvggc+9bP9PnTDOjsx9YrpD5VY4MRVY1GyRqb8dlP/t8InNU
1RD0LRf5YV0X2HqyPj8Ze97Q85Ac8zpnb4QAtVyXa0BwJko/O4uBdEFA81Tkt/0q
hqpkR/P/M5OtaXNj0k1WMUDtyLFRcv5UMbYiooIx+gC2IVrC4I0+ORPHPiURPtMv
GLoaCHYLi36QBnaJmgnDYnse4TGF5pWNil1FAgMBAAGjUzBRMB0GA1UdDgQWBBTn
K7KpHqgVwSa8MY52hPcZRAnq5TAfBgNVHSMEGDAWgBTnK7KpHqgVwSa8MY52hPcZ
RAnq5TAPBgNVHRMBAf8EBTADAQH/MA0GCSqGSIb3DQEBCwUAA4IBAQABkpNERrsy
a35RhGOQx1Yy/uDoqXjekKocDhkADaoqJaDjnCusMf1HOK3H6UxR6k25cNfL4yGD
gSZMzpvdQY3YO7AvZlW3upPXgw7Ywhk5/ho1h/iuh5DlMwdjDDuqfVaUE0D8+hO/
70Qy0ocptAt/tv6r5McfQ1V/wSC2AHYAo4Mn3nqKf2rnJ1oRBVr1cz1MtkTfTqGN
hjTbfFbAUPyHWI+DVo7NncG48KbMyJKR3wKT2YbKaA0or8RHn11+t4eJ9+M4REkZ
V0aDiG3x3wgREmvCSosC1E9NTyqllPF5L1l6D+hngc2Xda9dRAjPMbDxp6rezb7y
MEDoShOvx94i
-----END CERTIFICATE-----";

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
                    handle.setopt_ptr_owned(curl_sys::CURLOPT_DEBUGDATA, [0_u8; 1])?;
                    let mut aliases = nyquest_backend_curl::CurlStringList::default();
                    aliases.append("200 OK");
                    handle.setopt_slist(curl_sys::CURLOPT_HTTP200ALIASES, aliases)?;
                    handle.setopt_blob(curl_sys::CURLOPT_CAINFO_BLOB, MOCK_CA_BUNDLE)?;
                    handle.setopt_long(curl_sys::CURLOPT_TCP_KEEPALIVE, 1)?;
                    handle.setopt_off_t(curl_sys::CURLOPT_MAX_RECV_SPEED_LARGE, 1_000_000)?;
                    let mut response_code = 0;
                    assert_eq!(
                        curl_sys::curl_easy_getinfo(
                            handle.as_raw(),
                            curl_sys::CURLINFO_RESPONSE_CODE,
                            &mut response_code,
                        ),
                        curl_sys::CURLE_OK
                    );
                    assert_eq!(response_code, 0);
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
