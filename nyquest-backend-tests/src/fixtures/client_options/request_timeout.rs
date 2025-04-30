#[cfg(test)]
mod tests {
    use http_body_util::Full;
    use nyquest::Error;
    use nyquest::Request as NyquestRequest;

    use crate::*;

    const BODY: &str = "1234567890";

    async fn delayed_response_handler() -> FixtureAssertionResult {
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
        let res = Response::new(Full::new(Bytes::from(BODY)));
        (res.into(), Ok(()))
    }

    #[test]
    fn test_request_timeout() {
        const PATH: &str = "client_options/request_timeout";

        let _handle = crate::add_hyper_fixture(PATH, |_| delayed_response_handler());

        #[cfg(feature = "blocking")]
        {
            let builder = crate::init_builder_blocking()
                .unwrap()
                .request_timeout(std::time::Duration::from_secs(1));
            let client = builder.build_blocking().unwrap();
            let err = client
                .request(NyquestRequest::get(PATH))
                .and_then(|r| r.text())
                .unwrap_err();
            assert!(matches!(err, Error::RequestTimeout));
        }

        #[cfg(feature = "async")]
        {
            TOKIO_RT.block_on(async {
                let builder = crate::init_builder()
                    .await
                    .unwrap()
                    .request_timeout(std::time::Duration::from_secs(1));
                let client = builder.build_async().await.unwrap();
                let res = client.request(NyquestRequest::get(PATH)).await;
                assert!(matches!(res.unwrap_err(), Error::RequestTimeout));
            });
        }
    }

    #[test]
    fn test_request_didnt_timeout() {
        const PATH: &str = "client_options/request_didnt_timeout";

        let _handle = crate::add_hyper_fixture(PATH, |_| delayed_response_handler());

        #[cfg(feature = "blocking")]
        {
            let builder = crate::init_builder_blocking()
                .unwrap()
                .request_timeout(std::time::Duration::from_secs(10));
            let client = builder.build_blocking().unwrap();
            let res = client
                .request(NyquestRequest::get(PATH))
                .unwrap()
                .text()
                .unwrap();
            assert_eq!(res, BODY);
        }

        #[cfg(feature = "async")]
        {
            TOKIO_RT.block_on(async {
                let builder = crate::init_builder()
                    .await
                    .unwrap()
                    .request_timeout(std::time::Duration::from_secs(10));
                let client = builder.build_async().await.unwrap();
                let res = client
                    .request(NyquestRequest::get(PATH))
                    .await
                    .unwrap()
                    .text()
                    .await
                    .unwrap();
                assert_eq!(res, BODY);
            });
        }
    }
}
