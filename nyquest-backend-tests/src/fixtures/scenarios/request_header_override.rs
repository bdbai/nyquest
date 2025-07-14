#[cfg(test)]
mod tests {
    use http_body_util::Full;
    #[cfg(feature = "blocking")]
    use nyquest::blocking::Body as NyquestBlockingBody;
    #[cfg(feature = "async")]
    use nyquest::r#async::Body as NyquestAsyncBody;
    use nyquest::Request as NyquestRequest;

    use crate::*;

    #[test]
    fn test_request_header_override() {
        const PATH: &str = "scenarios/request_header_override";
        const DEFAULT_HEADER_NAME: &str = "x-default-header";
        const DEFAULT_HEADER_VALUE: &str = "default";
        const EXPECTED_HEADER_VALUE: &str = "expected";
        const BODY_TEXT: &str = "Hello, plain text!";

        let _handle = crate::add_hyper_fixture(PATH, {
            move |req| async move {
                let content_type_headers: Vec<_> = req
                    .headers()
                    .get_all(DEFAULT_HEADER_NAME)
                    .iter()
                    .map(|h| h.to_str().unwrap_or_default())
                    .collect();
                let content_type_headers = content_type_headers.join("; ");

                let response_body = Bytes::from(content_type_headers.into_bytes());
                let res = Response::new(Full::new(response_body));

                (res, Ok(()))
            }
        });

        let assertions = |received_content_type: &str| {
            assert_eq!(received_content_type, EXPECTED_HEADER_VALUE);
        };

        #[cfg(feature = "blocking")]
        {
            let builder = crate::init_builder_blocking()
                .unwrap()
                .with_header(DEFAULT_HEADER_NAME, DEFAULT_HEADER_VALUE);
            let client = builder.build_blocking().unwrap();
            let res = client
                .request(
                    NyquestRequest::post(PATH)
                        .with_body(NyquestBlockingBody::plain_text(BODY_TEXT))
                        .with_header(DEFAULT_HEADER_NAME, EXPECTED_HEADER_VALUE),
                )
                .unwrap();
            let content_type = res.text().unwrap();
            assertions(&content_type);
        }

        #[cfg(feature = "async")]
        {
            let content_type = TOKIO_RT.block_on(async {
                let builder = crate::init_builder()
                    .await
                    .unwrap()
                    .with_header(DEFAULT_HEADER_NAME, DEFAULT_HEADER_VALUE);
                let client = builder.build_async().await.unwrap();
                let res = client
                    .request(
                        NyquestRequest::post(PATH)
                            .with_body(NyquestAsyncBody::plain_text(BODY_TEXT))
                            .with_header(DEFAULT_HEADER_NAME, EXPECTED_HEADER_VALUE),
                    )
                    .await
                    .unwrap();
                res.text().await.unwrap()
            });
            assertions(&content_type);
        }
    }
}
