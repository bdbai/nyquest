#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use http_body_util::Full;
    use hyper::body;
    use hyper::header::{CACHE_CONTROL, ETAG};
    use hyper::Request;
    use hyper::Response;
    use nyquest::Request as NyquestRequest;

    use crate::*;

    const ETAG_VALUE: &str = "\"abcdef123456\"";
    const RESPONSE_TEXT: &str = "This is a cacheable response";

    // Helper function to create a cacheable response handler
    async fn create_cacheable_handler<'a>(
        counters: Arc<[AtomicUsize; 2]>,
        req: Request<body::Incoming>,
    ) -> FixtureAssertionResult {
        let is_blocking = req.is_blocking() as usize;
        if req.method().clone() == hyper::Method::GET {
            counters[is_blocking].fetch_add(1, Ordering::SeqCst);
        }

        let res = Response::builder()
            .header(CACHE_CONTROL, "max-age=3600, public")
            .header(ETAG, ETAG_VALUE)
            .body(Full::new(hyper::body::Bytes::from(RESPONSE_TEXT)))
            .unwrap();

        (res.into(), Ok(()))
    }

    #[cfg(not(feature = "curl"))] // libcurl does not support caching
    #[test]
    fn test_response_caching() {
        const PATH: &str = "client_options/response_caching";

        let request_counters = Arc::new([AtomicUsize::new(0), AtomicUsize::new(0)]);

        let _handle = crate::add_hyper_fixture(PATH, {
            let counter = request_counters.clone();
            move |req| create_cacheable_handler(counter.clone(), req)
        });

        let assertions = |counter: &AtomicUsize, res1: String, res2: String| {
            assert_eq!(res1, RESPONSE_TEXT);
            assert_eq!(res2, RESPONSE_TEXT);

            let counter = counter.load(Ordering::SeqCst);
            let total_counter = request_counters[0].load(Ordering::SeqCst)
                + request_counters[1].load(Ordering::SeqCst);
            if counter != 1 && total_counter != 1 {
                panic!(
                    "Expected 1 request, but got {}, {} in total",
                    counter, total_counter
                );
            }
        };
        #[cfg(feature = "blocking")]
        {
            let builder = crate::init_builder_blocking().unwrap();
            let client = builder.build_blocking().unwrap();

            let res1 = client
                .request(NyquestRequest::get(PATH))
                .unwrap()
                .text()
                .unwrap();

            let res2 = client
                .request(NyquestRequest::get(PATH))
                .unwrap()
                .text()
                .unwrap();

            assertions(&request_counters[1], res1, res2);
        }

        // Test the async client
        #[cfg(feature = "async")]
        {
            let (res1, res2) = TOKIO_RT.block_on(async {
                let builder = crate::init_builder().await.unwrap();
                let client = builder.build_async().await.unwrap();

                let res1 = client
                    .request(NyquestRequest::get(PATH))
                    .await
                    .unwrap()
                    .text()
                    .await
                    .unwrap();

                let res2 = client
                    .request(NyquestRequest::get(PATH))
                    .await
                    .unwrap()
                    .text()
                    .await
                    .unwrap();

                (res1, res2)
            });

            assertions(&request_counters[0], res1, res2);
        }
    }

    #[test]
    fn test_no_caching() {
        const PATH: &str = "client_options/no_caching";

        let request_counters = Arc::new([AtomicUsize::new(0), AtomicUsize::new(0)]);

        let _handle = crate::add_hyper_fixture(PATH, {
            let counter = request_counters.clone();
            move |req| create_cacheable_handler(counter.clone(), req)
        });

        let assertions = |counter: &AtomicUsize, res1: String, res2: String| {
            assert_eq!(res1, RESPONSE_TEXT);
            assert_eq!(res2, RESPONSE_TEXT);

            assert_eq!(counter.load(Ordering::SeqCst), 2);
        };
        #[cfg(feature = "blocking")]
        {
            let builder = crate::init_builder_blocking().unwrap().no_caching();
            let client = builder.build_blocking().unwrap();

            let res1 = client
                .request(NyquestRequest::get(PATH))
                .unwrap()
                .text()
                .unwrap();

            let res2 = client
                .request(NyquestRequest::get(PATH))
                .unwrap()
                .text()
                .unwrap();

            assertions(&request_counters[1], res1, res2);
        }

        #[cfg(feature = "async")]
        {
            let (res1, res2) = TOKIO_RT.block_on(async {
                let builder = crate::init_builder().await.unwrap().no_caching();
                let client = builder.build_async().await.unwrap();

                let res1 = client
                    .request(NyquestRequest::get(PATH))
                    .await
                    .unwrap()
                    .text()
                    .await
                    .unwrap();

                let res2 = client
                    .request(NyquestRequest::get(PATH))
                    .await
                    .unwrap()
                    .text()
                    .await
                    .unwrap();

                (res1, res2)
            });

            assertions(&request_counters[0], res1, res2);
        }
    }
}
