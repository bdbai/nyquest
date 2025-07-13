#[cfg(test)]
mod tests {
    use http_body_util::Full;
    use hyper::header::{COOKIE, SET_COOKIE};
    use hyper::{body, Request, Response};
    #[cfg(feature = "blocking")]
    use nyquest::blocking::Body as NyquestBlockingBody;
    #[cfg(feature = "async")]
    use nyquest::r#async::Body as NyquestAsyncBody;
    use nyquest::Request as NyquestRequest;

    use crate::*;

    const COOKIE_NAME: &str = "TEST_COOKIE";
    const MOCK_COOKIE_VALUE: &str = "mock_cookie_value";

    async fn cookie_handler(req: Request<body::Incoming>) -> FixtureAssertionResult {
        let request_cookie_value = req
            .headers()
            .get(COOKIE)
            .and_then(|v| {
                v.to_str()
                    .ok()?
                    .strip_prefix(COOKIE_NAME)?
                    .strip_prefix('=')
            })
            .unwrap_or_default();
        let response_body = Bytes::copy_from_slice(request_cookie_value.as_bytes());

        let request_body = req.into_body().collect().await.unwrap().to_bytes();
        let request_body = String::from_utf8_lossy(&request_body);
        let set_cookie_header_value = format!("{COOKIE_NAME}={request_body}; Path=/");

        let res = Response::builder()
            .header(SET_COOKIE, set_cookie_header_value)
            .body(Full::new(response_body))
            .unwrap();
        (res.into(), Ok(()))
    }

    #[test]
    fn test_cookies_enabled() {
        const PATH: &str = "client_options/cookies_enabled";

        let _handle = crate::add_hyper_fixture(PATH, cookie_handler);

        let assertions = |response_body: String| {
            assert_eq!(response_body, MOCK_COOKIE_VALUE);
        };

        #[cfg(feature = "blocking")]
        {
            let builder = crate::init_builder_blocking().unwrap();
            let client = builder.build_blocking().unwrap();

            let request = NyquestRequest::post(PATH)
                .with_body(NyquestBlockingBody::plain_text(MOCK_COOKIE_VALUE));
            client.request(request).unwrap(); // First request to set the cookie

            let request = NyquestRequest::post(PATH);
            let response_body = client.request(request).unwrap().text().unwrap();
            assertions(response_body);
        }

        #[cfg(feature = "async")]
        {
            let response_body = TOKIO_RT.block_on(async {
                let builder = crate::init_builder().await.unwrap();
                let client = builder.build_async().await.unwrap();

                let request = NyquestRequest::post(PATH)
                    .with_body(NyquestAsyncBody::plain_text(MOCK_COOKIE_VALUE));
                client.request(request).await.unwrap(); // First request to set the cookie

                let request = NyquestRequest::post(PATH);
                client.request(request).await.unwrap().text().await.unwrap()
            });
            assertions(response_body);
        }
    }

    #[test]
    fn test_cookies_disabled() {
        const PATH: &str = "client_options/cookies_disabled";

        let _handle = crate::add_hyper_fixture(PATH, cookie_handler);

        let assertions = |response_body: String| {
            assert_eq!(response_body, "");
        };

        #[cfg(feature = "blocking")]
        {
            let builder = crate::init_builder_blocking().unwrap().no_cookies();
            let client = builder.build_blocking().unwrap();

            let request = NyquestRequest::post(PATH)
                .with_body(NyquestBlockingBody::plain_text(MOCK_COOKIE_VALUE));
            client.request(request).unwrap(); // First request to set the cookie

            let request = NyquestRequest::post(PATH);
            let response_body = client.request(request).unwrap().text().unwrap();
            assertions(response_body);
        }

        #[cfg(feature = "async")]
        {
            let response_body = TOKIO_RT.block_on(async {
                let builder = crate::init_builder().await.unwrap().no_cookies();
                let client = builder.build_async().await.unwrap();

                let request = NyquestRequest::post(PATH)
                    .with_body(NyquestAsyncBody::plain_text(MOCK_COOKIE_VALUE));
                client.request(request).await.unwrap(); // First request to set the cookie

                let request = NyquestRequest::post(PATH);
                client.request(request).await.unwrap().text().await.unwrap()
            });
            assertions(response_body);
        }
    }
}
