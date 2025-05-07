#[cfg(test)]
mod tests {
    use http_body_util::Full;
    use hyper::header::CONTENT_TYPE;
    use nyquest::{Error, Request as NyquestRequest};

    use crate::*;

    #[test]
    fn test_json_deserialization_error() {
        const PATH: &str = "errors/invalid_json_response";
        const INVALID_JSON: &str = r#"{"name": "Test User", "age": 30, invalid_json}"#;

        let _handle = crate::add_hyper_fixture(PATH, |_| async move {
            let res = Response::builder()
                .header(CONTENT_TYPE, "application/json")
                .body(Full::new(Bytes::from(INVALID_JSON)))
                .unwrap();
            (res, Ok(()))
        });

        #[allow(unused)]
        #[derive(serde::Deserialize, Debug)]
        struct User {
            name: String,
            age: u32,
        }

        let builder = crate::init_builder_blocking().unwrap();

        #[cfg(feature = "blocking")]
        {
            let client = builder.clone().build_blocking().unwrap();
            let err = client
                .request(NyquestRequest::get(PATH))
                .unwrap()
                .json::<User>()
                .unwrap_err();

            assert!(matches!(err, Error::Json(_)));
        }

        #[cfg(feature = "async")]
        {
            let err = TOKIO_RT.block_on(async {
                let client = builder.build_async().await.unwrap();
                client
                    .request(NyquestRequest::get(PATH))
                    .await
                    .unwrap()
                    .json::<User>()
                    .await
                    .unwrap_err()
            });

            assert!(matches!(err, Error::Json(_)));
        }
    }
}
