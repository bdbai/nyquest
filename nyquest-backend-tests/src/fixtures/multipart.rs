#[cfg(test)]
mod tests {
    use std::{
        io::Cursor,
        pin::pin,
        sync::{Arc, OnceLock},
        task::{Context, Poll},
    };

    use futures::{task::noop_waker_ref, StreamExt as _};
    use http_body_util::{BodyExt, BodyStream};
    use hyper::{
        body::Incoming,
        header::{HeaderValue, CONTENT_TYPE},
    };
    use multer::Multipart;
    #[cfg(feature = "blocking")]
    use nyquest::blocking::Body as NyquestBlockingBody;
    #[cfg(feature = "async")]
    use nyquest::r#async::Body as NyquestAsyncBody;
    use nyquest::Request as NyquestRequest;
    use nyquest::{Part, PartBody};

    use crate::*;

    const TEST_CONTENT: &str = "test content";

    #[test]
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
                            file_name: field.file_name().unwrap_or("not_a_file").into(),
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
                    file_name: "not_a_file".into(),
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
                    file_name: "not_a_file".into(),
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

    async fn echo_server_fixture(
        req: Request<Incoming>,
    ) -> (Response<Full<Bytes>>, Result<(), Request<Incoming>>) {
        let req_content_type = req
            .headers()
            .get(CONTENT_TYPE)
            .cloned()
            .unwrap_or(HeaderValue::from_static(""));
        let body = req.into_body().collect().await.unwrap().to_bytes();
        let mut res = Response::new(Full::from(body));
        res.headers_mut()
            .insert("req-content-type", req_content_type);
        (res, Ok(()))
    }

    fn multipart_body_assertions(content_type: Option<String>, body: String) {
        fn poll_once<F: Future<Output = T>, T>(fut: F) -> T {
            let fut = pin!(fut);
            let poll = fut.poll(&mut Context::from_waker(noop_waker_ref()));
            match poll {
                Poll::Ready(val) => val,
                Poll::Pending => panic!("Future did not resolve immediately"),
            }
        }

        let boundary = multer::parse_boundary(content_type.unwrap()).unwrap();
        let mut multipart = Multipart::new(
            futures_util::stream::once(async { Ok::<_, io::Error>(Bytes::from(body)) }),
            boundary,
        );

        let field = poll_once(multipart.next_field())
            .expect("Failed to get field")
            .expect("Field not found");
        assert_eq!(field.name(), Some("file"));
        assert_eq!(field.file_name(), Some("test.txt"));
        assert_eq!(field.content_type().unwrap().to_string(), "text/plain");
        let content = poll_once(field.bytes()).expect("Failed to read field bytes");
        assert_eq!(content, TEST_CONTENT.as_bytes());
        assert!(poll_once(multipart.next_field())
            .expect("Failed to get next field")
            .is_none());
    }

    #[test]
    #[cfg(any(feature = "blocking-stream", feature = "async-stream"))]
    fn test_multipart_upload() {
        const PATH: &str = "requests/multipart_upload";

        let _handle = crate::add_hyper_fixture(PATH, echo_server_fixture);

        #[cfg(feature = "blocking")]
        {
            let builder = crate::init_builder_blocking().unwrap();
            let client = builder.build_blocking().unwrap();
            let body = NyquestBlockingBody::multipart([Part::new_with_content_type(
                "file",
                "text/plain",
                PartBody::stream(Cursor::new(TEST_CONTENT), TEST_CONTENT.len() as u64),
            )
            .with_filename("test.txt")]);
            let response = client
                .request(NyquestRequest::post(PATH).with_body(body))
                .unwrap();
            multipart_body_assertions(
                response
                    .get_header("req-content-type")
                    .unwrap()
                    .into_iter()
                    .next(),
                response.text().unwrap(),
            );
        }

        #[cfg(feature = "async")]
        {
            let (content_type, res) = TOKIO_RT.block_on(async {
                let builder = crate::init_builder().await.unwrap();
                let client = builder.build_async().await.unwrap();
                let body = NyquestAsyncBody::multipart([Part::new_with_content_type(
                    "file",
                    "text/plain",
                    PartBody::stream(
                        futures_util::io::Cursor::new(TEST_CONTENT),
                        TEST_CONTENT.len() as u64,
                    ),
                )
                .with_filename("test.txt")]);
                let response = client
                    .request(NyquestRequest::post(PATH).with_body(body))
                    .await
                    .unwrap();
                let content_type = response
                    .get_header("req-content-type")
                    .unwrap()
                    .into_iter()
                    .next();
                (content_type, response.text().await.unwrap())
            });
            multipart_body_assertions(content_type, res);
        }
    }

    #[test]
    #[cfg(any(feature = "blocking-stream", feature = "async-stream"))]

    fn test_multipart_unsized_upload() {
        const PATH: &str = "requests/multipart_unsized_upload";

        let _handle = crate::add_hyper_fixture(PATH, echo_server_fixture);

        #[cfg(feature = "blocking")]
        {
            let builder = crate::init_builder_blocking().unwrap();
            let client = builder.build_blocking().unwrap();
            let body = NyquestBlockingBody::multipart([Part::new_with_content_type(
                "file",
                "text/plain",
                PartBody::stream_unsized(Cursor::new(TEST_CONTENT)),
            )
            .with_filename("test.txt")]);
            let response = client
                .request(NyquestRequest::post(PATH).with_body(body))
                .unwrap();
            multipart_body_assertions(
                response
                    .get_header("req-content-type")
                    .unwrap()
                    .into_iter()
                    .next(),
                response.text().unwrap(),
            );
        }

        #[cfg(feature = "async")]
        {
            let (content_type, res) = TOKIO_RT.block_on(async {
                let builder = crate::init_builder().await.unwrap();
                let client = builder.build_async().await.unwrap();
                let body = NyquestAsyncBody::multipart([Part::new_with_content_type(
                    "file",
                    "text/plain",
                    PartBody::stream_unsized(futures_util::io::Cursor::new(TEST_CONTENT)),
                )
                .with_filename("test.txt")]);
                let response = client
                    .request(NyquestRequest::post(PATH).with_body(body))
                    .await
                    .unwrap();
                let content_type = response
                    .get_header("req-content-type")
                    .unwrap()
                    .into_iter()
                    .next();
                (content_type, response.text().await.unwrap())
            });
            multipart_body_assertions(content_type, res);
        }
    }
}
