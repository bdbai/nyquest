#[cfg(test)]
mod tests {
    use std::{
        ops::Deref,
        sync::{Arc, OnceLock},
    };

    use futures::StreamExt as _;
    use http_body_util::{BodyExt, BodyStream};
    use hyper::header::{ACCEPT, CONTENT_LANGUAGE, CONTENT_TYPE};
    use hyper::{Method, StatusCode};
    use memchr::memmem;
    use multer::Multipart;
    #[cfg(feature = "blocking")]
    use nyquest::blocking::Body as NyquestBlockingBody;
    #[cfg(feature = "async")]
    use nyquest::r#async::Body as NyquestAsyncBody;
    use nyquest::{body_form, Request as NyquestRequest};
    #[cfg(feature = "multipart")]
    use nyquest::{Part, PartBody};

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

                let header_values = format!("{accept}|{content_lang}");
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
    #[cfg(feature = "multipart")]
    fn test_body_multipart_bytes() {
        const PATH: &str = "requests/body_multipart_bytes";
        #[derive(Debug, Clone, Default, PartialEq, Eq)]
        struct FormItem {
            name: String,
            file_name: String,
            content_type: String,
            bytes: Bytes,
            content_lang: Option<String>,
        }
        let received_facts = Arc::new([const { OnceLock::new() }; 2]);
        let _handle = crate::add_hyper_fixture(PATH, {
            let received_body = Arc::clone(&received_facts);
            move |req: Request<body::Incoming>| {
                let received_body = Arc::clone(&received_body);
                async move {
                    let boundary = req
                        .headers()
                        .get(CONTENT_TYPE)
                        .and_then(|ct| ct.to_str().ok())
                        .and_then(|ct| multer::parse_boundary(ct).ok());
                    let is_blocking = req.is_blocking();
                    let content_type = req
                        .headers()
                        .get(CONTENT_TYPE)
                        .map(|v| v.to_str().unwrap().to_owned());

                    let body_stream =
                        BodyStream::new(req.into_body()).filter_map(|result| async move {
                            result.map(|frame| frame.into_data().ok()).transpose()
                        });

                    let mut multipart = Multipart::new(body_stream, boundary.unwrap_or_default());
                    let mut form_items = vec![];
                    while let Some(field) = multipart.next_field().await.unwrap() {
                        form_items.push(FormItem {
                            name: field.name().unwrap_or_default().to_owned(),
                            file_name: field.file_name().unwrap_or_default().into(),
                            content_type: field
                                .content_type()
                                .map(|mime| mime.to_string())
                                .unwrap_or_default(),
                            content_lang: field
                                .headers()
                                .get("content-language")
                                .map(|v| v.to_str().unwrap_or_default().to_owned()),
                            bytes: field.bytes().await.unwrap_or_default(),
                        });
                    }
                    received_body[is_blocking as usize]
                        .set((form_items, content_type))
                        .ok();
                    let res = Response::new(Full::new(Default::default()));
                    (res, Ok(()))
                }
            }
        });
        let assertions = |(items, content_type): &(Vec<FormItem>, Option<String>)| {
            assert!(content_type
                .as_deref()
                .unwrap()
                .starts_with("multipart/form-data; "),);
            assert_eq!(items.len(), 3);
            assert_eq!(
                items[0],
                FormItem {
                    name: "text".to_owned(),
                    file_name: "".into(),
                    content_type: "text/plain".to_owned(),
                    bytes: Bytes::from_static(b"ttt"),
                    content_lang: None,
                }
            );
            assert_eq!(
                items[1],
                FormItem {
                    name: "filename".to_owned(),
                    file_name: "3253212.mp3".into(),
                    content_type: "audio/mpeg".to_owned(),
                    bytes: Bytes::from_static(b"ID3"),
                    content_lang: None,
                }
            );
            assert_eq!(
                items[2],
                FormItem {
                    name: "headed".to_owned(),
                    file_name: "".into(),
                    content_type: "text/plain".to_owned(),
                    bytes: Bytes::from_static(b"head"),
                    content_lang: Some("zh-CN".to_owned()),
                }
            );
        };
        #[cfg(feature = "blocking")]
        {
            let builder = crate::init_builder_blocking().unwrap();
            let req_body = NyquestRequest::post(PATH).with_body(NyquestBlockingBody::multipart([
                Part::new_with_content_type("text", "text/plain", PartBody::text("ttt")),
                Part::new_with_content_type("filename", "audio/mpeg", PartBody::bytes(b"ID3"))
                    .with_filename("3253212.mp3"),
                Part::new_with_content_type("headed", "text/plain", PartBody::text("head"))
                    .with_header("content-language", "zh-CN"),
            ]));
            let client = builder.build_blocking().unwrap();
            client.request(req_body).unwrap();
            assertions(received_facts[1].get().unwrap());
        }
        #[cfg(feature = "async")]
        {
            let req_body = NyquestRequest::post(PATH).with_body(NyquestAsyncBody::multipart([
                Part::new_with_content_type("text", "text/plain", PartBody::text("ttt")),
                Part::new_with_content_type("filename", "audio/mpeg", PartBody::bytes(b"ID3"))
                    .with_filename("3253212.mp3"),
                Part::new_with_content_type(
                    "headed",
                    "text/plain",
                    PartBody::text("head".to_string()),
                )
                .with_header("content-language", "zh-CN"),
            ]));
            TOKIO_RT.block_on(async move {
                let builder = crate::init_builder().await.unwrap();
                let client = builder.build_async().await.unwrap();
                client.request(req_body).await.unwrap();
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
    fn test_head() {
        const PATH: &str = "requests/head";
        let _handle = crate::add_hyper_fixture(PATH, {
            move |req| async move {
                let mut res = Response::new(Full::default());
                if req.method() != Method::HEAD {
                    *res.status_mut() = StatusCode::BAD_REQUEST;
                }
                (res, Ok(()))
            }
        });

        #[cfg(feature = "blocking")]
        {
            let builder = crate::init_builder_blocking().unwrap();
            let client = builder.build_blocking().unwrap();
            let response = client.request(NyquestRequest::head(PATH)).unwrap();
            assert_eq!(response.status(), 200);
        }

        #[cfg(feature = "async")]
        {
            TOKIO_RT.block_on(async {
                let builder = crate::init_builder().await.unwrap();
                let client = builder.build_async().await.unwrap();
                let response = client.request(NyquestRequest::head(PATH)).await.unwrap();
                assert_eq!(response.status(), 200);
            });
        }
    }
}
