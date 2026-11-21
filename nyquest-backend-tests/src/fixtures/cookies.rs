#[cfg(test)]
#[cfg(not(feature = "winhttp"))] // WinHTTP does not support cookies management of its internal cookie store
mod tests {
    use hyper::header::{COOKIE, SET_COOKIE};
    use nyquest::{Cookie, Request as NyquestRequest};

    use crate::*;

    const URL: &str = "https://example.com/path/page";
    const OTHER_URL: &str = "https://other.example.org/path/page";

    fn find_cookie<'a>(cookies: &'a [Cookie], name: &str) -> Option<&'a Cookie> {
        cookies.iter().find(|cookie| cookie.name == name)
    }

    fn expected_host_from_base_url(base_url: &str) -> String {
        base_url
            .split("://")
            .nth(1)
            .unwrap_or(base_url)
            .split([':', '/'])
            .next()
            .unwrap_or_default()
            .to_owned()
    }

    fn assert_domain_eq(actual: &str, expected: &str) {
        // `localhost.` (FQDN with trailing dot, used to avoid proxy bypass)
        // is normalized to `localhost` by some platform cookie stores
        // (notably NSHTTPCookieStorage). Trailing-dot differences are not
        // semantically meaningful for cookie domain matching.
        assert_eq!(
            actual.trim_end_matches('.'),
            expected.trim_end_matches('.'),
            "cookie domain mismatch"
        );
    }

    fn assert_cookie(cookies: &[Cookie], name: &str, value: &str) {
        let cookie = find_cookie(cookies, name).unwrap_or_else(|| {
            panic!("cookie {name} not found in {cookies:?}");
        });
        assert_eq!(cookie.value, value);
        assert_eq!(cookie.domain, "example.com");
        assert_eq!(cookie.path, "/path");
        assert_eq!(cookie.expires, None);
        assert!(!cookie.is_http_only);
        assert!(!cookie.is_secure);
    }

    #[test]
    fn test_add_and_get_cookies() {
        #[cfg(feature = "blocking")]
        {
            let client = crate::init_builder_blocking()
                .unwrap()
                .build_blocking()
                .unwrap();

            client
                .add_cookie("TEST_COOKIE", "mock_cookie_value", URL)
                .unwrap();
            let cookies = client.get_cookies_for(URL).unwrap();
            assert_cookie(&cookies, "TEST_COOKIE", "mock_cookie_value");

            let other = client.get_cookies_for(OTHER_URL).unwrap();
            assert!(find_cookie(&other, "TEST_COOKIE").is_none());
        }

        #[cfg(feature = "async")]
        {
            crate::TOKIO_RT.block_on(async {
                let client = crate::init_builder()
                    .await
                    .unwrap()
                    .build_async()
                    .await
                    .unwrap();

                client
                    .add_cookie("TEST_COOKIE", "mock_cookie_value", URL)
                    .await
                    .unwrap();
                let cookies = client.get_cookies_for(URL).await.unwrap();
                assert_cookie(&cookies, "TEST_COOKIE", "mock_cookie_value");

                let other = client.get_cookies_for(OTHER_URL).await.unwrap();
                assert!(find_cookie(&other, "TEST_COOKIE").is_none());
            });
        }
    }

    #[test]
    fn test_cookies_are_filtered_by_path() {
        #[cfg(feature = "blocking")]
        {
            let client = crate::init_builder_blocking()
                .unwrap()
                .build_blocking()
                .unwrap();

            client
                .add_cookie("FIRST_COOKIE", "first_value", URL)
                .unwrap();
            client
                .add_cookie("SECOND_COOKIE", "second_value", URL)
                .unwrap();

            let cookies = client.get_cookies_for(URL).unwrap();
            assert_cookie(&cookies, "FIRST_COOKIE", "first_value");
            assert_cookie(&cookies, "SECOND_COOKIE", "second_value");

            let elsewhere_on_same_host =
                client.get_cookies_for("https://example.com/other").unwrap();
            assert!(find_cookie(&elsewhere_on_same_host, "FIRST_COOKIE").is_none());

            let under_same_path = client
                .get_cookies_for("https://example.com/path/other")
                .unwrap();
            assert_cookie(&under_same_path, "FIRST_COOKIE", "first_value");
        }

        #[cfg(feature = "async")]
        {
            crate::TOKIO_RT.block_on(async {
                let client = crate::init_builder()
                    .await
                    .unwrap()
                    .build_async()
                    .await
                    .unwrap();

                client
                    .add_cookie("FIRST_COOKIE", "first_value", URL)
                    .await
                    .unwrap();
                client
                    .add_cookie("SECOND_COOKIE", "second_value", URL)
                    .await
                    .unwrap();

                let cookies = client.get_cookies_for(URL).await.unwrap();
                assert_cookie(&cookies, "FIRST_COOKIE", "first_value");
                assert_cookie(&cookies, "SECOND_COOKIE", "second_value");

                let elsewhere_on_same_host = client
                    .get_cookies_for("https://example.com/other")
                    .await
                    .unwrap();
                assert!(find_cookie(&elsewhere_on_same_host, "FIRST_COOKIE").is_none());

                let under_same_path = client
                    .get_cookies_for("https://example.com/path/other")
                    .await
                    .unwrap();
                assert_cookie(&under_same_path, "FIRST_COOKIE", "first_value");
            });
        }
    }

    #[cfg(feature = "idn")]
    #[test]
    fn test_idna_cookie_domain_matching() {
        // `münchen.de` (Unicode) and `xn--mnchen-3ya.de` (punycode) are the same
        // host and must share cookies after IDNA normalization.
        const UNICODE_URL: &str = "https://münchen.de/path/page";
        const PUNYCODE_URL: &str = "https://xn--mnchen-3ya.de/path/page";
        const PUNYCODE_DOMAIN: &str = "xn--mnchen-3ya.de";

        #[cfg(feature = "blocking")]
        {
            let client = crate::init_builder_blocking()
                .unwrap()
                .build_blocking()
                .unwrap();

            client
                .add_cookie("IDNA_COOKIE", "idna_value", UNICODE_URL)
                .unwrap();

            let via_punycode = client.get_cookies_for(PUNYCODE_URL).unwrap();
            let cookie = find_cookie(&via_punycode, "IDNA_COOKIE").unwrap_or_else(|| {
                panic!("IDNA cookie not found via punycode URL in {via_punycode:?}");
            });
            assert_eq!(cookie.value, "idna_value");
            assert_eq!(cookie.domain, PUNYCODE_DOMAIN);
            assert_eq!(cookie.path, "/path");
            assert_eq!(cookie.expires, None);
            assert!(!cookie.is_http_only);
            assert!(!cookie.is_secure);

            let via_unicode = client.get_cookies_for(UNICODE_URL).unwrap();
            assert!(find_cookie(&via_unicode, "IDNA_COOKIE").is_some());

            let other = client
                .get_cookies_for("https://other.example.org/path/page")
                .unwrap();
            assert!(find_cookie(&other, "IDNA_COOKIE").is_none());

            // Reverse direction: add via punycode, read via Unicode.
            let client2 = crate::init_builder_blocking()
                .unwrap()
                .build_blocking()
                .unwrap();
            client2
                .add_cookie("IDNA_COOKIE_REV", "rev_value", PUNYCODE_URL)
                .unwrap();
            let via_unicode_rev = client2.get_cookies_for(UNICODE_URL).unwrap();
            let rev_cookie =
                find_cookie(&via_unicode_rev, "IDNA_COOKIE_REV").unwrap_or_else(|| {
                    panic!("IDNA cookie not found via Unicode URL in {via_unicode_rev:?}");
                });
            assert_eq!(rev_cookie.value, "rev_value");
            assert_eq!(rev_cookie.domain, PUNYCODE_DOMAIN);
            assert_eq!(rev_cookie.path, "/path");
            assert_eq!(rev_cookie.expires, None);
            assert!(!rev_cookie.is_http_only);
            assert!(!rev_cookie.is_secure);
        }

        #[cfg(feature = "async")]
        {
            crate::TOKIO_RT.block_on(async {
                let client = crate::init_builder()
                    .await
                    .unwrap()
                    .build_async()
                    .await
                    .unwrap();

                client
                    .add_cookie("IDNA_COOKIE", "idna_value", UNICODE_URL)
                    .await
                    .unwrap();

                let via_punycode = client.get_cookies_for(PUNYCODE_URL).await.unwrap();
                let cookie = find_cookie(&via_punycode, "IDNA_COOKIE").unwrap_or_else(|| {
                    panic!("IDNA cookie not found via punycode URL in {via_punycode:?}");
                });
                assert_eq!(cookie.value, "idna_value");
                assert_eq!(cookie.domain, PUNYCODE_DOMAIN);
                assert_eq!(cookie.path, "/path");
                assert_eq!(cookie.expires, None);
                assert!(!cookie.is_http_only);
                assert!(!cookie.is_secure);

                let via_unicode = client.get_cookies_for(UNICODE_URL).await.unwrap();
                assert!(find_cookie(&via_unicode, "IDNA_COOKIE").is_some());

                let client2 = crate::init_builder()
                    .await
                    .unwrap()
                    .build_async()
                    .await
                    .unwrap();
                client2
                    .add_cookie("IDNA_COOKIE_REV", "rev_value", PUNYCODE_URL)
                    .await
                    .unwrap();
                let via_unicode_rev = client2.get_cookies_for(UNICODE_URL).await.unwrap();
                let rev_cookie =
                    find_cookie(&via_unicode_rev, "IDNA_COOKIE_REV").unwrap_or_else(|| {
                        panic!("IDNA cookie not found via Unicode URL in {via_unicode_rev:?}");
                    });
                assert_eq!(rev_cookie.value, "rev_value");
                assert_eq!(rev_cookie.domain, PUNYCODE_DOMAIN);
                assert_eq!(rev_cookie.path, "/path");
                assert_eq!(rev_cookie.expires, None);
                assert!(!rev_cookie.is_http_only);
                assert!(!rev_cookie.is_secure);
            });
        }
    }

    #[test]
    fn test_added_cookie_is_sent_with_request() {
        const PATH_ECHO: &str = "cookies/send_echo";
        const PATH_OUTSIDE: &str = "unrelated/send_echo";
        const COOKIE_NAME: &str = "SEND_COOKIE";
        const COOKIE_VALUE: &str = "send_value";

        let _handle_echo = crate::add_hyper_fixture(PATH_ECHO, |req| async move {
            let received = req
                .headers()
                .get(COOKIE)
                .and_then(|v| v.to_str().ok())
                .unwrap_or_default()
                .to_owned();
            let res = Response::new(Full::new(Bytes::from(received.into_bytes())));
            (res, Ok(()))
        });
        let _handle_outside = crate::add_hyper_fixture(PATH_OUTSIDE, |req| async move {
            let received = req
                .headers()
                .get(COOKIE)
                .and_then(|v| v.to_str().ok())
                .unwrap_or_default()
                .to_owned();
            let res = Response::new(Full::new(Bytes::from(received.into_bytes())));
            (res, Ok(()))
        });

        #[cfg(feature = "blocking")]
        {
            let base_url = crate::service_base_url_blocking().unwrap();
            let cookie_url = format!("{base_url}/{PATH_ECHO}");
            let client = crate::init_builder_blocking()
                .unwrap()
                .build_blocking()
                .unwrap();

            client
                .add_cookie(COOKIE_NAME, COOKIE_VALUE, cookie_url.clone())
                .unwrap();

            let echoed = client
                .request(NyquestRequest::get(PATH_ECHO))
                .unwrap()
                .text()
                .unwrap();
            assert!(
                echoed.contains(&format!("{COOKIE_NAME}={COOKIE_VALUE}")),
                "added cookie was not sent with matching request, got Cookie header: {echoed:?}"
            );

            let outside = client
                .request(NyquestRequest::get(PATH_OUTSIDE))
                .unwrap()
                .text()
                .unwrap();
            assert!(
                !outside.contains(COOKIE_NAME),
                "cookie leaked into non-matching path request, got Cookie header: {outside:?}"
            );
        }

        #[cfg(feature = "async")]
        {
            crate::TOKIO_RT.block_on(async {
                let base_url = crate::service_base_url().await.unwrap();
                let cookie_url = format!("{base_url}/{PATH_ECHO}");
                let client = crate::init_builder()
                    .await
                    .unwrap()
                    .build_async()
                    .await
                    .unwrap();

                client
                    .add_cookie(COOKIE_NAME, COOKIE_VALUE, cookie_url.clone())
                    .await
                    .unwrap();

                let echoed = client
                    .request(NyquestRequest::get(PATH_ECHO))
                    .await
                    .unwrap()
                    .text()
                    .await
                    .unwrap();
                assert!(
                    echoed.contains(&format!("{COOKIE_NAME}={COOKIE_VALUE}")),
                    "added cookie was not sent with matching request, got Cookie header: {echoed:?}"
                );

                let outside = client
                    .request(NyquestRequest::get(PATH_OUTSIDE))
                    .await
                    .unwrap()
                    .text()
                    .await
                    .unwrap();
                assert!(
                    !outside.contains(COOKIE_NAME),
                    "cookie leaked into non-matching path request, got Cookie header: {outside:?}"
                );
            });
        }
    }

    #[test]
    fn test_manually_added_cookie_is_scoped_on_redirect() {
        const START_OUTSIDE: &str = "cookies/redirect_leak_start";
        const TARGET_OUTSIDE: &str = "unrelated/redirect_leak_target";
        const START_INSIDE: &str = "cookies/redirect_keep_start";
        const TARGET_INSIDE: &str = "cookies/redirect_keep_target";
        const LEAK_COOKIE: &str = "REDIRECT_LEAK_COOKIE";
        const LEAK_VALUE: &str = "leak_value";
        const KEEP_COOKIE: &str = "REDIRECT_KEEP_COOKIE";
        const KEEP_VALUE: &str = "keep_value";

        async fn echo_cookies(req: Request<body::Incoming>) -> FixtureAssertionResult {
            let received = req
                .headers()
                .get(COOKIE)
                .and_then(|v| v.to_str().ok())
                .unwrap_or_default()
                .to_owned();
            let res = Response::new(Full::new(Bytes::from(received.into_bytes())));
            (res.into(), Ok(()))
        }

        let _handle_start_outside = crate::add_hyper_fixture(START_OUTSIDE, |_req| async move {
            let res: crate::hyper_fixture_collection::ResponseWrapper = Response::builder()
                .header("Location", format!("/{TARGET_OUTSIDE}"))
                .status(302)
                .body(Full::new(Bytes::new()))
                .unwrap()
                .into();
            (res, Ok(()))
        });
        let _handle_target_outside = crate::add_hyper_fixture(TARGET_OUTSIDE, echo_cookies);
        let _handle_start_inside = crate::add_hyper_fixture(START_INSIDE, |_req| async move {
            let res: crate::hyper_fixture_collection::ResponseWrapper = Response::builder()
                .header("Location", format!("/{TARGET_INSIDE}"))
                .status(302)
                .body(Full::new(Bytes::new()))
                .unwrap()
                .into();
            (res, Ok(()))
        });
        let _handle_target_inside = crate::add_hyper_fixture(TARGET_INSIDE, echo_cookies);

        #[cfg(feature = "blocking")]
        {
            let base_url = crate::service_base_url_blocking().unwrap();
            let client = crate::init_builder_blocking()
                .unwrap()
                .build_blocking()
                .unwrap();

            client
                .add_cookie(
                    LEAK_COOKIE,
                    LEAK_VALUE,
                    format!("{base_url}/{START_OUTSIDE}"),
                )
                .unwrap();
            client
                .add_cookie(
                    KEEP_COOKIE,
                    KEEP_VALUE,
                    format!("{base_url}/{START_INSIDE}"),
                )
                .unwrap();

            let leaked = client
                .request(NyquestRequest::get(START_OUTSIDE))
                .unwrap()
                .text()
                .unwrap();
            assert!(
                !leaked.contains(LEAK_COOKIE),
                "manually added cookie leaked to redirect target with non-matching path, got Cookie header: {leaked:?}"
            );

            let kept = client
                .request(NyquestRequest::get(START_INSIDE))
                .unwrap()
                .text()
                .unwrap();
            assert!(
                kept.contains(&format!("{KEEP_COOKIE}={KEEP_VALUE}")),
                "manually added cookie was not sent to redirect target with matching path, got Cookie header: {kept:?}"
            );
        }

        #[cfg(feature = "async")]
        {
            crate::TOKIO_RT.block_on(async {
                let base_url = crate::service_base_url().await.unwrap();
                let client = crate::init_builder()
                    .await
                    .unwrap()
                    .build_async()
                    .await
                    .unwrap();

                client
                    .add_cookie(
                        LEAK_COOKIE,
                        LEAK_VALUE,
                        format!("{base_url}/{START_OUTSIDE}"),
                    )
                    .await
                    .unwrap();
                client
                    .add_cookie(
                        KEEP_COOKIE,
                        KEEP_VALUE,
                        format!("{base_url}/{START_INSIDE}"),
                    )
                    .await
                    .unwrap();

                let leaked = client
                    .request(NyquestRequest::get(START_OUTSIDE))
                    .await
                    .unwrap()
                    .text()
                    .await
                    .unwrap();
                assert!(
                    !leaked.contains(LEAK_COOKIE),
                    "manually added cookie leaked to redirect target with non-matching path, got Cookie header: {leaked:?}"
                );

                let kept = client
                    .request(NyquestRequest::get(START_INSIDE))
                    .await
                    .unwrap()
                    .text()
                    .await
                    .unwrap();
                assert!(
                    kept.contains(&format!("{KEEP_COOKIE}={KEEP_VALUE}")),
                    "manually added cookie was not sent to redirect target with matching path, got Cookie header: {kept:?}"
                );
            });
        }
    }

    #[test]
    fn test_set_cookie_is_stored_and_readable() {
        const PATH_SET: &str = "cookies/store_set";
        const COOKIE_NAME: &str = "STORE_COOKIE";
        const COOKIE_VALUE: &str = "store_value";

        let _handle = crate::add_hyper_fixture(PATH_SET, |_req| async move {
            let mut res = Response::new(Full::new(Bytes::new()));
            res.headers_mut().insert(
                SET_COOKIE,
                format!("{COOKIE_NAME}={COOKIE_VALUE}; Path=/cookies")
                    .parse()
                    .unwrap(),
            );
            (res, Ok(()))
        });

        #[cfg(feature = "blocking")]
        {
            let base_url = crate::service_base_url_blocking().unwrap();
            let matching_url = format!("{base_url}/cookies/store_echo");
            let outside_url = format!("{base_url}/unrelated/store_echo");
            let client = crate::init_builder_blocking()
                .unwrap()
                .build_blocking()
                .unwrap();

            client.request(NyquestRequest::get(PATH_SET)).unwrap();

            let matching = client.get_cookies_for(matching_url.clone()).unwrap();
            let stored = find_cookie(&matching, COOKIE_NAME).unwrap_or_else(|| {
                panic!("Set-Cookie not readable via matching URL in {matching:?}");
            });
            assert_eq!(stored.value, COOKIE_VALUE);
            assert_domain_eq(&stored.domain, &expected_host_from_base_url(&base_url));
            assert_eq!(stored.path, "/cookies");
            assert_eq!(stored.expires, None);
            assert!(!stored.is_http_only);
            assert!(!stored.is_secure);

            let outside = client.get_cookies_for(outside_url.clone()).unwrap();
            assert!(
                find_cookie(&outside, COOKIE_NAME).is_none(),
                "Set-Cookie leaked into non-matching path URL: {outside:?}"
            );

            let other_host = client
                .get_cookies_for("https://other.example.org/path/page")
                .unwrap();
            assert!(find_cookie(&other_host, COOKIE_NAME).is_none());
        }

        #[cfg(feature = "async")]
        {
            crate::TOKIO_RT.block_on(async {
                let base_url = crate::service_base_url().await.unwrap();
                let matching_url = format!("{base_url}/cookies/store_echo");
                let outside_url = format!("{base_url}/unrelated/store_echo");
                let client = crate::init_builder()
                    .await
                    .unwrap()
                    .build_async()
                    .await
                    .unwrap();

                client.request(NyquestRequest::get(PATH_SET)).await.unwrap();

                let matching = client.get_cookies_for(matching_url.clone()).await.unwrap();
                let stored = find_cookie(&matching, COOKIE_NAME).unwrap_or_else(|| {
                    panic!("Set-Cookie not readable via matching URL in {matching:?}");
                });
                assert_eq!(stored.value, COOKIE_VALUE);
                assert_domain_eq(&stored.domain, &expected_host_from_base_url(&base_url));
                assert_eq!(stored.path, "/cookies");
                assert_eq!(stored.expires, None);
                assert!(!stored.is_http_only);
                assert!(!stored.is_secure);

                let outside = client.get_cookies_for(outside_url.clone()).await.unwrap();
                assert!(
                    find_cookie(&outside, COOKIE_NAME).is_none(),
                    "Set-Cookie leaked into non-matching path URL: {outside:?}"
                );

                let other_host = client
                    .get_cookies_for("https://other.example.org/path/page")
                    .await
                    .unwrap();
                assert!(find_cookie(&other_host, COOKIE_NAME).is_none());
            });
        }
    }

    #[test]
    fn test_set_cookie_attributes_are_exposed() {
        const PATH_SET: &str = "cookies/store_attrs";
        const COOKIE_NAME: &str = "ATTR_COOKIE";
        const COOKIE_VALUE: &str = "attr_value";

        let _handle = crate::add_hyper_fixture(PATH_SET, |_req| async move {
            let mut res = Response::new(Full::new(Bytes::new()));
            res.headers_mut().insert(
                SET_COOKIE,
                format!(
                    "{COOKIE_NAME}={COOKIE_VALUE}; Path=/cookies; \
                     Expires=Mon, 21 Oct 2030 07:28:00 GMT; HttpOnly"
                )
                .parse()
                .unwrap(),
            );
            (res, Ok(()))
        });

        fn assert_attrs(cookies: &[Cookie], base_url: &str) {
            let stored = find_cookie(cookies, COOKIE_NAME).unwrap_or_else(|| {
                panic!("Set-Cookie with attributes not readable in {cookies:?}");
            });
            assert_eq!(stored.value, COOKIE_VALUE);
            assert_domain_eq(&stored.domain, &expected_host_from_base_url(base_url));
            assert_eq!(stored.path, "/cookies");
            // `Secure` was not set, so it must read back as false on every backend.
            assert!(!stored.is_secure);
            let expires = stored.expires.unwrap_or_else(|| {
                panic!("expected persistent cookie to expose expires in {stored:?}");
            });
            assert!(
                expires > std::time::SystemTime::now(),
                "expires should be in the future, got {expires:?}"
            );
            assert!(stored.is_http_only, "expected HttpOnly in {stored:?}");
        }

        #[cfg(feature = "blocking")]
        {
            let base_url = crate::service_base_url_blocking().unwrap();
            let matching_url = format!("{base_url}/cookies/store_echo");
            let client = crate::init_builder_blocking()
                .unwrap()
                .build_blocking()
                .unwrap();

            client.request(NyquestRequest::get(PATH_SET)).unwrap();

            let matching = client.get_cookies_for(matching_url.clone()).unwrap();
            assert_attrs(&matching, &base_url);
        }

        #[cfg(feature = "async")]
        {
            crate::TOKIO_RT.block_on(async {
                let base_url = crate::service_base_url().await.unwrap();
                let matching_url = format!("{base_url}/cookies/store_echo");
                let client = crate::init_builder()
                    .await
                    .unwrap()
                    .build_async()
                    .await
                    .unwrap();

                client.request(NyquestRequest::get(PATH_SET)).await.unwrap();

                let matching = client.get_cookies_for(matching_url.clone()).await.unwrap();
                assert_attrs(&matching, &base_url);
            });
        }
    }
}
