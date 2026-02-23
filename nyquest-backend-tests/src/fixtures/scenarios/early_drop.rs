#[cfg(test)]
mod tests {
    use hyper::Response;
    use nyquest::Request as NyquestRequest;

    use crate::*;

    #[test]
    fn test_early_drop_client() {
        const PATH: &str = "scenarios/early_drop/client";

        let _handle = crate::add_hyper_fixture(PATH, {
            move |_req| {
                async move {
                    // Convert the stream to a boxed body
                    let res = Response::new(Full::new(Bytes::from_static(b"ok")));

                    (res, Ok(()))
                }
            }
        });

        let assertion = |content: Result<String, nyquest::Error>| {
            assert_eq!(content.unwrap(), "ok");
        };
        #[cfg(feature = "blocking")]
        {
            let builder = crate::init_builder_blocking().unwrap();
            let client = builder.build_blocking().unwrap();
            let res = client.request(NyquestRequest::get(PATH)).unwrap();
            drop(client);
            let res = res.with_successful_status().unwrap();
            assertion(res.text());
        }

        #[cfg(feature = "async")]
        {
            TOKIO_RT.block_on(async {
                let builder = crate::init_builder().await.unwrap();
                let client = builder.build_async().await.unwrap();
                let res = client.request(NyquestRequest::get(PATH)).await.unwrap();
                drop(client);
                let res = res.with_successful_status().unwrap();
                assertion(res.text().await);
            });
        }
    }
}
