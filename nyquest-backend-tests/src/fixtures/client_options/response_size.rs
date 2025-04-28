#[cfg(test)]
mod tests {
    use http_body_util::Full;
    use nyquest::Error;
    use nyquest::Request as NyquestRequest;

    use crate::*;

    const BODY: &str = "1234567890"; // 10 bytes

    async fn static_response_handler() -> FixtureAssertionResult {
        let res = Response::new(Full::new(Bytes::from(BODY)));
        (res.into(), Ok(()))
    }

    #[test]
    fn test_response_within_limit() {
        const PATH: &str = "client_options/response_within_limit";

        let _handle = crate::add_hyper_fixture(PATH, |_| static_response_handler());

        let assertions = |content: String| {
            assert_eq!(content, BODY);
        };

        #[cfg(feature = "blocking")]
        {
            let builder = crate::init_builder_blocking()
                .unwrap()
                .max_response_buffer_size(10);
            let client = builder.build_blocking().unwrap();
            let res = client
                .request(NyquestRequest::get(PATH))
                .unwrap()
                .text()
                .unwrap();
            assertions(res);
        }

        #[cfg(feature = "async")]
        {
            let res = TOKIO_RT.block_on(async {
                let builder = crate::init_builder()
                    .await
                    .unwrap()
                    .max_response_buffer_size(10);
                let client = builder.build_async().await.unwrap();
                client
                    .request(NyquestRequest::get(PATH))
                    .await
                    .unwrap()
                    .text()
                    .await
                    .unwrap()
            });
            assertions(res);
        }
    }

    #[test]
    fn test_response_exceeds_limit() {
        const PATH: &str = "client_options/response_exceeds_limit";

        let _handle = crate::add_hyper_fixture(PATH, |_| static_response_handler());

        #[cfg(feature = "blocking")]
        {
            let builder = crate::init_builder_blocking()
                .unwrap()
                .max_response_buffer_size(9);
            let client = builder.build_blocking().unwrap();
            let err = client
                .request(NyquestRequest::get(PATH))
                .and_then(|r| r.text())
                .unwrap_err();
            assert!(matches!(err, Error::ResponseTooLarge));
        }

        #[cfg(feature = "async")]
        {
            let res = TOKIO_RT.block_on(async {
                let builder = crate::init_builder()
                    .await
                    .unwrap()
                    .max_response_buffer_size(9);
                let client = builder.build_async().await.unwrap();
                let res = client.request(NyquestRequest::get(PATH)).await?;
                res.text().await
            });
            assert!(matches!(res.unwrap_err(), Error::ResponseTooLarge));
        }
    }
}
