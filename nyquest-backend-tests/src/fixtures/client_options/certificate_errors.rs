#[cfg(test)]
mod tests {
    use std::time::Duration;

    use nyquest::Request as NyquestRequest;

    use crate::*;

    #[test]
    fn test_certificate_expired() {
        const TARGET_URL: &str = "https://expired.badssl.com/";
        const TIMEOUT: Duration = Duration::from_secs(30);

        #[cfg(feature = "blocking")]
        {
            let builder = crate::init_builder_blocking().unwrap();
            let client = builder.request_timeout(TIMEOUT).build_blocking().unwrap();

            client.request(NyquestRequest::get(TARGET_URL)).unwrap_err();
        }

        #[cfg(feature = "async")]
        {
            let res = TOKIO_RT.block_on(async {
                let builder = crate::init_builder().await.unwrap();
                let client = builder
                    .request_timeout(TIMEOUT)
                    .build_async()
                    .await
                    .unwrap();

                client.request(NyquestRequest::get(TARGET_URL)).await
            });
            res.unwrap_err();
        }
    }

    #[test]
    fn test_certificate_expired_ignored() {
        const TARGET_URL: &str = "https://expired.badssl.com/";

        #[cfg(feature = "blocking")]
        {
            let builder = crate::init_builder_blocking()
                .unwrap()
                .dangerously_ignore_certificate_errors();
            let client = builder.build_blocking().unwrap();

            let res = client.request(NyquestRequest::get(TARGET_URL)).unwrap();
            res.with_successful_status().unwrap();
        }

        #[cfg(feature = "async")]
        {
            let res = TOKIO_RT.block_on(async {
                let builder = crate::init_builder()
                    .await
                    .unwrap()
                    .dangerously_ignore_certificate_errors();
                let client = builder.build_async().await.unwrap();

                client
                    .request(NyquestRequest::get(TARGET_URL))
                    .await
                    .unwrap()
                    .with_successful_status()
            });
            res.unwrap();
        }
    }
}
