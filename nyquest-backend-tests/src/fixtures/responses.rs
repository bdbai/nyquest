#[cfg(test)]
mod tests {
    use http_body_util::BodyExt;
    use hyper::{Method, StatusCode};
    #[cfg(feature = "blocking")]
    use nyquest::blocking::Body as NyquestBlockingBody;
    #[cfg(feature = "async")]
    use nyquest::r#async::Body as NyquestAsyncBody;
    use nyquest::Request as NyquestRequest;

    use crate::*;

    #[test]
    fn test_get_text() {
        const PATH: &str = "responses/get_text";
        const BODY: &str = r#"{"message": "Hello, world!"}"#;
        let _handle = crate::add_hyper_fixture(PATH, |req| async move {
            let res = Response::new(Full::new(Bytes::from(BODY)));
            (res, (req.method() == Method::GET).then_some(()).ok_or(req))
        });
        let builder = crate::init_builder_blocking().unwrap();
        let assertions = |(status, content_len, content)| {
            assert_eq!(status, 200);
            assert_eq!(content_len, Some(BODY.len() as u64));
            assert_eq!(content, BODY);
        };
        #[cfg(feature = "blocking")]
        {
            let client = builder.clone().build_blocking().unwrap();
            let res = client.request(NyquestRequest::get(PATH)).unwrap();
            let status = res.status();
            let content_len = res.content_length();
            let content = res.text().unwrap();
            assertions((status, content_len, content));
        }
        #[cfg(feature = "async")]
        {
            let facts = TOKIO_RT.block_on(async {
                let client = builder.build_async().await.unwrap();
                let res = client.request(NyquestRequest::get(PATH)).await.unwrap();
                let status = res.status();
                let content_len = res.content_length();
                (status, content_len, res.text().await.unwrap())
            });
            assertions(facts);
        }
    }

    #[test]
    fn test_get_bytes() {
        const PATH: &str = "responses/get_bytes";
        const BODY: &[u8] = b"\x01\x02\x03\x04";
        let _handle = crate::add_hyper_fixture(PATH, |req| async move {
            let res = Response::new(Full::new(Bytes::from(BODY)));
            (res, (req.method() == Method::GET).then_some(()).ok_or(req))
        });
        let builder = crate::init_builder_blocking().unwrap();
        let assertions = |(status, content_len, content)| {
            assert_eq!(status, 200);
            assert_eq!(content_len, Some(BODY.len() as u64));
            assert_eq!(content, BODY);
        };
        #[cfg(feature = "blocking")]
        {
            let client = builder.clone().build_blocking().unwrap();
            let res = client.request(NyquestRequest::get(PATH)).unwrap();
            let status = res.status();
            let content_len = res.content_length();
            let content = res.bytes().unwrap();
            assertions((status, content_len, content));
        }
        #[cfg(feature = "async")]
        {
            let facts = TOKIO_RT.block_on(async {
                let client = builder.build_async().await.unwrap();
                let res = client.request(NyquestRequest::get(PATH)).await.unwrap();
                let status = res.status();
                let content_len = res.content_length();
                (status, content_len, res.bytes().await.unwrap())
            });
            assertions(facts);
        }
    }

    #[test]
    fn test_status_codes() {
        const PATH: &str = "responses/status_codes";
        const STATUS_CODES: [u16; 4] = [400, 404, 500, 502];
        let _handle = crate::add_hyper_fixture(PATH, |mut req| async move {
            let mut res = Response::<Full<Bytes>>::default();
            let body = req.body_mut().collect().await.ok().and_then(|bytes| {
                let status = String::from_utf8_lossy(&bytes.to_bytes()).parse().ok()?;
                StatusCode::from_u16(status).ok()
            });
            match body {
                Some(status) => {
                    *res.status_mut() = status;
                    (res, Ok(()))
                }
                None => (res, Err(req)),
            }
        });
        let builder = crate::init_builder_blocking().unwrap();
        #[cfg(feature = "blocking")]
        let blocking_client = builder.clone().build_blocking().unwrap();
        #[cfg(feature = "async")]
        let async_client =
            TOKIO_RT.block_on(async move { builder.clone().build_async().await.unwrap() });
        for expected_status_code in STATUS_CODES {
            let assertions = |actual_status_code| {
                assert_eq!(actual_status_code, expected_status_code);
            };
            let body_text = expected_status_code.to_string();
            let request_mime = "text/plain";
            #[cfg(feature = "blocking")]
            {
                let request = NyquestRequest::post(PATH)
                    .with_body(NyquestBlockingBody::text(body_text.clone(), request_mime));
                let res = blocking_client.request(request).unwrap();
                let status = res.status();
                assertions(status);
            }
            #[cfg(feature = "async")]
            {
                let request = NyquestRequest::post(PATH)
                    .with_body(NyquestAsyncBody::text(body_text, request_mime));
                let status = TOKIO_RT.block_on(async {
                    let res = async_client.request(request).await.unwrap();
                    let status = res.status();
                    status
                });
                assertions(status);
            }
        }
    }

    #[test]
    fn test_get_header() {
        const PATH: &str = "responses/get_header";
        const HEADER_NAME: &str = "X-Test-Header";
        const HEADER_VALUE: &str = "test-value";
        let _handle = crate::add_hyper_fixture(PATH, |_req| async move {
            let mut res = Response::<Full<Bytes>>::default();
            res.headers_mut()
                .insert(HEADER_NAME, HEADER_VALUE.parse().unwrap());
            (res, Ok(()))
        });
        let builder = crate::init_builder_blocking().unwrap();
        let assertions = |header_value: Option<String>| {
            assert_eq!(header_value.as_deref(), Some(HEADER_VALUE));
        };
        #[cfg(feature = "blocking")]
        {
            let client = builder.clone().build_blocking().unwrap();
            let res = client.request(NyquestRequest::get(PATH)).unwrap();
            let header_value = res.get_header(HEADER_NAME).unwrap().pop();
            assertions(header_value);
        }
        #[cfg(feature = "async")]
        {
            let facts = TOKIO_RT.block_on(async {
                let client = builder.build_async().await.unwrap();
                let res = client.request(NyquestRequest::get(PATH)).await.unwrap();
                let header_value = res.get_header(HEADER_NAME).unwrap().pop();
                header_value
            });
            assertions(facts);
        }
    }
}
