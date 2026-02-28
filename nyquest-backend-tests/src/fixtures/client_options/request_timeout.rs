#[cfg(test)]
mod tests {
    use http_body_util::Full;
    use nyquest::Error;
    use nyquest::Request as NyquestRequest;

    use crate::*;

    const BODY: &str = "1234567890";

    async fn delayed_response_handler(secs: u64) -> FixtureAssertionResult {
        tokio::time::sleep(std::time::Duration::from_secs(secs)).await;
        let res = Response::new(Full::new(Bytes::from(BODY)));
        (res.into(), Ok(()))
    }

    #[test]
    fn test_request_timeout() {
        const PATH: &str = "client_options/request_timeout";

        let _handle = crate::add_hyper_fixture(PATH, |_| delayed_response_handler(30));

        #[cfg(feature = "blocking")]
        {
            let time_start = std::time::Instant::now();
            let builder = crate::init_builder_blocking()
                .unwrap()
                .request_timeout(std::time::Duration::from_secs(1));
            let client = builder.build_blocking().unwrap();
            let err = client
                .request(NyquestRequest::get(PATH))
                .and_then(|r| r.text())
                .unwrap_err();
            assert!(matches!(err, Error::RequestTimeout));
            assert!(time_start.elapsed() < std::time::Duration::from_secs(10));
        }

        #[cfg(feature = "async")]
        {
            let time_start = std::time::Instant::now();
            TOKIO_RT.block_on(async {
                let builder = crate::init_builder()
                    .await
                    .unwrap()
                    .request_timeout(std::time::Duration::from_secs(1));
                let client = builder.build_async().await.unwrap();
                let res = client.request(NyquestRequest::get(PATH)).await;
                assert!(matches!(res.unwrap_err(), Error::RequestTimeout));
            });
            assert!(time_start.elapsed() < std::time::Duration::from_secs(10));
        }
    }

    #[test]
    fn test_request_didnt_timeout() {
        const PATH: &str = "client_options/request_didnt_timeout";

        let _handle = crate::add_hyper_fixture(PATH, |_| delayed_response_handler(3));

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
