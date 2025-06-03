#[cfg(test)]
mod tests {
    use std::{
        io::Cursor,
        ops::Deref,
        sync::{Arc, OnceLock},
    };

    use http_body_util::BodyExt;
    use hyper::header::{ACCEPT, CONTENT_LANGUAGE, CONTENT_TYPE};
    use hyper::{Method, StatusCode};
    use memchr::memmem;
    #[cfg(feature = "blocking")]
    use nyquest::blocking::Body as NyquestBlockingBody;
    #[cfg(feature = "async")]
    use nyquest::r#async::Body as NyquestAsyncBody;
    use nyquest::{body_form, Request as NyquestRequest};

    use crate::*;

    #[test]
    fn test_headers() {
        const PATH: &str = "requests/headers";
        const ACCEPT_VALUE: &str = "application/json";
        const CONTENT_LANGUAGE_VALUE: &str = "en-US";

        let _handle = crate::add_hyper_fixture(PATH, {
            move |req| async move {
                let accept = req
                    .headers()
                    .get(ACCEPT)
                    .map(|v| v.to_str().unwrap_or_default().to_owned())
                    .unwrap_or_default();

                let content_lang = req
                    .headers()
                    .get(CONTENT_LANGUAGE)
                    .map(|v| v.to_str().unwrap_or_default().to_owned())
                    .unwrap_or_default();

                let header_values = format!("{}|{}", accept, content_lang);
                let response_body = Bytes::from(header_values.into_bytes());

                let res = Response::new(Full::new(response_body));
                (res, Ok(()))
            }
        });

        let assertions = |header_values: String| {
            let values: Vec<&str> = header_values.split('|').collect();
            assert_eq!(values.first().copied().unwrap_or_default(), ACCEPT_VALUE);
            assert_eq!(
                values.get(1).copied().unwrap_or_default(),
                CONTENT_LANGUAGE_VALUE
            );
        };

        #[cfg(feature = "blocking")]
        {
            let builder = crate::init_builder_blocking().unwrap();
            let client = builder.build_blocking().unwrap();
            let res = client
                .request(
                    NyquestRequest::post(PATH)
                        .with_header("Accept", ACCEPT_VALUE)
                        .with_header("Content-Language", CONTENT_LANGUAGE_VALUE)
                        .with_body(NyquestBlockingBody::plain_text("aa")),
                )
                .unwrap()
                .text()
                .unwrap();
            assertions(res);
        }

        #[cfg(feature = "async")]
        {
            let res = TOKIO_RT.block_on(async {
                let builder = crate::init_builder().await.unwrap();
                let client = builder.build_async().await.unwrap();
                client
                    .request(
                        NyquestRequest::post(PATH)
                            .with_header("Accept", ACCEPT_VALUE)
                            .with_header("Content-Language", CONTENT_LANGUAGE_VALUE)
                            .with_body(NyquestAsyncBody::plain_text("aa")),
                    )
                    .await
                    .unwrap()
                    .text()
                    .await
                    .unwrap()
            });
            assertions(res);
        }
    }

    fn double_deref<A: ?Sized, B: ?Sized>(
        t: &Option<(impl Deref<Target = A>, impl Deref<Target = B>)>,
    ) -> Option<(&A, &B)> {
        t.as_ref().map(|(a, b)| (&**a, &**b))
    }
    #[test]
    fn test_body_form() {
        const PATH: &str = "requests/body_form";
        const VALUE1: &str = "valu e1";
        const VALUE2: &str = "value=2哈";
        const VALUE3: &str = "val&&u e +3";
        let received_facts = Arc::new([const { OnceLock::new() }; 2]);
        let _handle = crate::add_hyper_fixture(PATH, {
            let received_body = Arc::clone(&received_facts);
            move |req: Request<body::Incoming>| {
                let received_body = Arc::clone(&received_body);
                async move {
                    let is_blocking = req.is_blocking();
                    let content_type = req
                        .headers()
                        .get(CONTENT_TYPE)
                        .map(|v| v.to_str().unwrap().to_owned());
                    let body = req.into_body().collect().await.unwrap().to_bytes();
                    received_body[is_blocking as usize]
                        .set((body, content_type))
                        .ok();
                    let res = Response::new(Full::new(Default::default()));
                    (res, Ok(()))
                }
            }
        });
        let assertions = |(bytes, content_type): &(Bytes, Option<String>)| {
            assert_eq!(
                content_type.as_deref(),
                Some("application/x-www-form-urlencoded")
            );
            let mut form = form_urlencoded::parse(bytes);
            assert_eq!(double_deref(&form.next()), Some(("key1", VALUE1)));
            assert_eq!(double_deref(&form.next()), Some(("key2", VALUE2)));
            assert_eq!(double_deref(&form.next()), Some(("key3", VALUE3)));
            assert_eq!(form.next().as_ref().map(|kv| &*kv.0), Some("key 4"));
            assert!(form.next().is_none());
            assert!(memmem::find(bytes, b"valu+e1").is_some());
        };
        #[cfg(feature = "blocking")]
        {
            let builder = crate::init_builder_blocking().unwrap();
            let req_body = body_form! {
                "key1" => VALUE1,
                "key2" => VALUE2,
                "key3" => VALUE3,
                "key 4" => "",
            };
            let client = builder.build_blocking().unwrap();
            client
                .request(NyquestRequest::post(PATH).with_body(req_body))
                .unwrap();
            assertions(received_facts[1].get().unwrap());
        }
        #[cfg(feature = "async")]
        {
            let req_body = body_form! {
                "key1" => VALUE1,
                "key2" => VALUE2,
                "key3" => VALUE3,
                "key 4" => "",
            };
            TOKIO_RT.block_on(async {
                let builder = crate::init_builder().await.unwrap();
                let client = builder.build_async().await.unwrap();
                client
                    .request(NyquestRequest::post(PATH).with_body(req_body))
                    .await
                    .unwrap();
            });
            assertions(received_facts[0].get().unwrap());
        }
    }

    #[test]
    fn test_put_without_body() {
        const PATH: &str = "requests/put_without_body";
        let _handle = crate::add_hyper_fixture(PATH, {
            move |req| async move {
                let mut res = Response::new(Full::default());
                if req.method() != Method::PUT {
                    *res.status_mut() = StatusCode::BAD_REQUEST;
                }
                (res, Ok(()))
            }
        });

        #[cfg(feature = "blocking")]
        {
            let builder = crate::init_builder_blocking().unwrap();
            let client = builder.build_blocking().unwrap();
            let response = client.request(NyquestRequest::put(PATH)).unwrap();
            assert_eq!(response.status(), 200);
        }

        #[cfg(feature = "async")]
        {
            TOKIO_RT.block_on(async {
                let builder = crate::init_builder().await.unwrap();
                let client = builder.build_async().await.unwrap();
                let response = client.request(NyquestRequest::put(PATH)).await.unwrap();
                assert_eq!(response.status(), 200);
            });
        }
    }

    #[test]
    fn test_stream_upload() {
        const PATH: &str = "requests/stream_upload";
        const CONTENT_TYPE: &str = "text/plain";
        const CONTENTS: &str = "1234567890";

        let _handle = crate::add_hyper_fixture(PATH, {
            move |req| async move {
                let content_type = req
                    .headers()
                    .get("content-type")
                    .map(|v| v.to_str().unwrap_or_default().to_owned())
                    .unwrap_or_default();
                let content_length = req
                    .headers()
                    .get("content-length")
                    .map(|v| v.to_str().unwrap_or_default().to_owned())
                    .unwrap_or_default();
                let mut res = Response::new(Full::default());
                if content_type != CONTENT_TYPE {
                    *res.status_mut() = StatusCode::UNPROCESSABLE_ENTITY;
                }
                if content_length != "10" {
                    *res.status_mut() = StatusCode::PAYLOAD_TOO_LARGE;
                }
                let body = req.into_body().collect().await.unwrap().to_bytes();
                if body != CONTENTS.as_bytes() {
                    *res.status_mut() = StatusCode::BAD_REQUEST;
                }
                (res, Ok(()))
            }
        });

        #[cfg(feature = "blocking")]
        {
            let builder = crate::init_builder_blocking().unwrap();
            let client = builder.build_blocking().unwrap();
            let body = NyquestBlockingBody::stream(Cursor::new(CONTENTS), CONTENT_TYPE, 10);
            let response = client
                .request(NyquestRequest::put(PATH).with_body(body))
                .unwrap();
            assert_eq!(response.status(), 200);
        }

        #[cfg(feature = "async")]
        {
            TOKIO_RT.block_on(async {
                let builder = crate::init_builder().await.unwrap();
                let client = builder.build_async().await.unwrap();
                let body = NyquestAsyncBody::stream(
                    futures_util::io::Cursor::new(CONTENTS),
                    CONTENT_TYPE,
                    10,
                );
                let response = client
                    .request(NyquestRequest::put(PATH).with_body(body))
                    .await
                    .unwrap();
                assert_eq!(response.status(), 200);
            });
        }
    }

    #[test]
    fn test_unsized_stream_upload() {
        const PATH: &str = "requests/unsized_stream_upload";
        const CONTENT_TYPE: &str = "text/plain";
        const CONTENTS: &str = "1234567890";

        let _handle = crate::add_hyper_fixture(PATH, {
            move |req| async move {
                let content_type = req
                    .headers()
                    .get("content-type")
                    .map(|v| v.to_str().unwrap_or_default().to_owned())
                    .unwrap_or_default();
                let mut res = Response::new(Full::default());
                if content_type != CONTENT_TYPE {
                    *res.status_mut() = StatusCode::UNPROCESSABLE_ENTITY;
                }
                let body = req.into_body().collect().await.unwrap().to_bytes();
                if body != CONTENTS.as_bytes() {
                    *res.status_mut() = StatusCode::BAD_REQUEST;
                }

                (res, Ok(()))
            }
        });

        #[cfg(feature = "blocking")]
        {
            let builder = crate::init_builder_blocking().unwrap();
            let client = builder.build_blocking().unwrap();
            let body = NyquestBlockingBody::stream_unsized(Cursor::new(CONTENTS), CONTENT_TYPE);
            let response = client
                .request(NyquestRequest::put(PATH).with_body(body))
                .unwrap();
            assert_eq!(response.status(), 200);
        }

        #[cfg(feature = "async")]
        {
            TOKIO_RT.block_on(async {
                let builder = crate::init_builder().await.unwrap();
                let client = builder.build_async().await.unwrap();
                let body = NyquestAsyncBody::stream_unsized(
                    futures_util::io::Cursor::new(CONTENTS),
                    CONTENT_TYPE,
                );
                let response = client
                    .request(NyquestRequest::put(PATH).with_body(body))
                    .await
                    .unwrap();
                assert_eq!(response.status(), 200);
            });
        }
    }
}
