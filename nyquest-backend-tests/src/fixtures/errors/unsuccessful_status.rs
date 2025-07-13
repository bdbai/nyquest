#[cfg(test)]
mod tests {
    use http_body_util::Full;
    use hyper::{Method, StatusCode};
    use nyquest::{Error, Request as NyquestRequest};

    use crate::*;

    #[test]
    fn test_unsuccessful_status_codes() {
        const PATH: &str = "errors/unsuccessful_status_codes";

        let _handle = crate::add_hyper_fixture(PATH, |req| async move {
            let mut res = Response::builder();

            if req.method() == Method::GET {
                res = res.status(StatusCode::BAD_REQUEST);
            } else if req.method() == Method::POST {
                res = res.status(StatusCode::NOT_FOUND);
            } else if req.method() == Method::PUT {
                res = res.status(StatusCode::INTERNAL_SERVER_ERROR);
            }

            let response = res.body(Full::new(Bytes::from("Error response"))).unwrap();
            (response, Ok(()))
        });

        let builder = crate::init_builder_blocking().unwrap();

        let assertions = |status_code: u16, error: Error| match error {
            Error::NonSuccessfulStatusCode(received_status) => {
                assert_eq!(received_status, status_code);
            }
            _ => panic!("Expected NonSuccessfulStatusCode error, got: {error:?}"),
        };

        #[cfg(feature = "blocking")]
        {
            let client = builder.clone().build_blocking().unwrap();

            // Test GET request (400 Bad Request)
            let err = client
                .request(NyquestRequest::get(PATH))
                .unwrap()
                .with_successful_status()
                .unwrap_err();
            assertions(400, err);

            // Test POST request (404 Not Found)
            let err = client
                .request(NyquestRequest::post(PATH))
                .unwrap()
                .with_successful_status()
                .unwrap_err();
            assertions(404, err);

            // Test PUT request (500 Internal Server Error)
            let err = client
                .request(NyquestRequest::put(PATH))
                .unwrap()
                .with_successful_status()
                .unwrap_err();
            assertions(500, err);
        }

        #[cfg(feature = "async")]
        {
            TOKIO_RT.block_on(async {
                let client = builder.build_async().await.unwrap();

                // Test GET request (400 Bad Request)
                let err = client
                    .request(NyquestRequest::get(PATH))
                    .await
                    .unwrap()
                    .with_successful_status()
                    .unwrap_err();
                assertions(400, err);

                // Test POST request (404 Not Found)
                let err = client
                    .request(NyquestRequest::post(PATH))
                    .await
                    .unwrap()
                    .with_successful_status()
                    .unwrap_err();
                assertions(404, err);

                // Test PUT request (500 Internal Server Error)
                let err = client
                    .request(NyquestRequest::put(PATH))
                    .await
                    .unwrap()
                    .with_successful_status()
                    .unwrap_err();
                assertions(500, err);
            });
        }
    }

    #[test]
    fn test_successful_status_codes() {
        const PATH: &str = "errors/successful_status_codes";

        let _handle = crate::add_hyper_fixture(PATH, |req| async move {
            let mut res = Response::builder();

            if req.method() == Method::GET {
                res = res.status(StatusCode::OK);
            } else if req.method() == Method::POST {
                res = res.status(StatusCode::CREATED);
            } else if req.method() == Method::PUT {
                res = res.status(StatusCode::NO_CONTENT);
            }

            let response = res
                .body(Full::new(Bytes::from("Success response")))
                .unwrap();
            (response, Ok(()))
        });

        let builder = crate::init_builder_blocking().unwrap();

        let assertions = |status_code: u16| {
            assert!((200..300).contains(&status_code));
        };

        #[cfg(feature = "blocking")]
        {
            let client = builder.clone().build_blocking().unwrap();

            // Test GET request (200 OK)
            let response = client
                .request(NyquestRequest::get(PATH))
                .unwrap()
                .with_successful_status()
                .unwrap();
            assertions(response.status().into());

            // Test POST request (201 Created)
            let response = client
                .request(NyquestRequest::post(PATH))
                .unwrap()
                .with_successful_status()
                .unwrap();
            assertions(response.status().into());

            // Test PUT request (204 No Content)
            let response = client
                .request(NyquestRequest::put(PATH))
                .unwrap()
                .with_successful_status()
                .unwrap();
            assertions(response.status().into());
        }

        #[cfg(feature = "async")]
        {
            TOKIO_RT.block_on(async {
                let client = builder.build_async().await.unwrap();

                // Test GET request (200 OK)
                let response = client
                    .request(NyquestRequest::get(PATH))
                    .await
                    .unwrap()
                    .with_successful_status()
                    .unwrap();
                assertions(response.status().into());

                // Test POST request (201 Created)
                let response = client
                    .request(NyquestRequest::post(PATH))
                    .await
                    .unwrap()
                    .with_successful_status()
                    .unwrap();
                assertions(response.status().into());

                // Test PUT request (204 No Content)
                let response = client
                    .request(NyquestRequest::put(PATH))
                    .await
                    .unwrap()
                    .with_successful_status()
                    .unwrap();
                assertions(response.status().into());
            });
        }
    }
}
