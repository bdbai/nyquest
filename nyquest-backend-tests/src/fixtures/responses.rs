#[cfg(test)]
mod tests {
    #[cfg(feature = "blocking")]
    use std::io::Read;
    use std::sync::Arc;

    #[cfg(feature = "async")]
    use futures::AsyncReadExt;
    use http_body_util::BodyExt;
    use hyper::{body::Frame, Method, StatusCode};
    #[cfg(feature = "blocking")]
    use nyquest::blocking::Body as NyquestBlockingBody;
    #[cfg(feature = "async")]
    use nyquest::r#async::Body as NyquestAsyncBody;
    use nyquest::Request as NyquestRequest;

    use crate::*;

    #[test]
    fn test_get_text() {
        const PATH: &str = "responses/get_text";
        const BODY: &str = r#"{"message": "Hello, world!"}"#;
        let _handle = crate::add_hyper_fixture(PATH, |req| async move {
            let res = Response::new(Full::new(Bytes::from(BODY)));
            (res, (req.method() == Method::GET).then_some(()).ok_or(req))
        });
        let builder = crate::init_builder_blocking().unwrap();
        let assertions = |(status, content_len, content): (u16, Option<u64>, String)| {
            assert_eq!(status, 200);
            assert_eq!(content_len, Some(BODY.len() as u64));
            assert_eq!(content, BODY);
        };
        #[cfg(feature = "blocking")]
        {
            let client = builder.clone().build_blocking().unwrap();
            let res = client
                .request(NyquestRequest::get(PATH))
                .unwrap()
                .with_successful_status()
                .unwrap();
            let status = res.status();
            let content_len = res.content_length();
            let content = res.text().unwrap();
            assertions((status.into(), content_len, content));
        }
        #[cfg(feature = "async")]
        {
            let facts = TOKIO_RT.block_on(async {
                let client = builder.build_async().await.unwrap();
                let res = client
                    .request(NyquestRequest::get(PATH))
                    .await
                    .unwrap()
                    .with_successful_status()
                    .unwrap();
                let status = res.status();
                let content_len = res.content_length();
                (status.into(), content_len, res.text().await.unwrap())
            });
            assertions(facts);
        }
    }

    #[test]
    fn test_get_bytes() {
        const PATH: &str = "responses/get_bytes";
        const BODY: &[u8] = b"\x01\x02\x03\x04";
        let _handle = crate::add_hyper_fixture(PATH, |req| async move {
            let res = Response::new(Full::new(Bytes::from(BODY)));
            (res, (req.method() == Method::GET).then_some(()).ok_or(req))
        });
        let builder = crate::init_builder_blocking().unwrap();
        let assertions = |(status, content_len, content): (u16, Option<u64>, Vec<u8>)| {
            assert_eq!(status, 200);
            assert_eq!(content_len, Some(BODY.len() as u64));
            assert_eq!(content, BODY);
        };
        #[cfg(feature = "blocking")]
        {
            let client = builder.clone().build_blocking().unwrap();
            let res = client.request(NyquestRequest::get(PATH)).unwrap();
            let status = res.status();
            let content_len = res.content_length();
            let content = res.bytes().unwrap();
            assertions((status.into(), content_len, content));
        }
        #[cfg(feature = "async")]
        {
            let facts = TOKIO_RT.block_on(async {
                let client = builder.build_async().await.unwrap();
                let res = client.request(NyquestRequest::get(PATH)).await.unwrap();
                let status = res.status();
                let content_len = res.content_length();
                (status.into(), content_len, res.bytes().await.unwrap())
            });
            assertions(facts);
        }
    }

    #[test]
    fn test_status_codes() {
        const PATH: &str = "responses/status_codes";
        const STATUS_CODES: [u16; 4] = [400, 404, 500, 502];
        let _handle = crate::add_hyper_fixture(PATH, |mut req| async move {
            let mut res = Response::<Full<Bytes>>::default();
            let body = req.body_mut().collect().await.ok().and_then(|bytes| {
                let status = String::from_utf8_lossy(&bytes.to_bytes()).parse().ok()?;
                StatusCode::from_u16(status).ok()
            });
            match body {
                Some(status) => {
                    *res.status_mut() = status;
                    (res, Ok(()))
                }
                None => (res, Err(req)),
            }
        });
        let builder = crate::init_builder_blocking().unwrap();
        #[cfg(feature = "blocking")]
        let blocking_client = builder.clone().build_blocking().unwrap();
        #[cfg(feature = "async")]
        let async_client =
            TOKIO_RT.block_on(async move { builder.clone().build_async().await.unwrap() });
        for expected_status_code in STATUS_CODES {
            let assertions = |actual_status_code: u16| {
                assert_eq!(actual_status_code, expected_status_code);
            };
            let body_text = expected_status_code.to_string();
            let request_mime = "text/plain";
            #[cfg(feature = "blocking")]
            {
                let request = NyquestRequest::post(PATH)
                    .with_body(NyquestBlockingBody::text(body_text.clone(), request_mime));
                let res = blocking_client.request(request).unwrap();
                let status = res.status().into();
                assertions(status);
            }
            #[cfg(feature = "async")]
            {
                let request = NyquestRequest::post(PATH)
                    .with_body(NyquestAsyncBody::text(body_text, request_mime));
                let status = TOKIO_RT.block_on(async {
                    let res = async_client.request(request).await.unwrap();
                    let status = res.status();
                    status.into()
                });
                assertions(status);
            }
        }
    }

    #[test]
    fn test_get_header() {
        const PATH: &str = "responses/get_header";
        const HEADER_NAME: &str = "X-Test-Header";
        const HEADER_VALUE: &str = "test-value";
        let _handle = crate::add_hyper_fixture(PATH, |_req| async move {
            let mut res = Response::<Full<Bytes>>::default();
            res.headers_mut()
                .insert(HEADER_NAME, HEADER_VALUE.parse().unwrap());
            (res, Ok(()))
        });
        let builder = crate::init_builder_blocking().unwrap();
        let assertions = |header_value: Option<String>| {
            assert_eq!(header_value.as_deref(), Some(HEADER_VALUE));
        };
        #[cfg(feature = "blocking")]
        {
            let client = builder.clone().build_blocking().unwrap();
            let res = client.request(NyquestRequest::get(PATH)).unwrap();
            let header_value = res.get_header(HEADER_NAME).unwrap().pop();
            assertions(header_value);
        }
        #[cfg(feature = "async")]
        {
            let facts = TOKIO_RT.block_on(async {
                let client = builder.build_async().await.unwrap();
                let res = client.request(NyquestRequest::get(PATH)).await.unwrap();
                res.get_header(HEADER_NAME).unwrap().pop()
            });
            assertions(facts);
        }
    }

    #[test]
    fn test_stream_download() {
        const PATH: &str = "responses/stream_download";

        let (mut async_tx, async_rx) = futures::channel::mpsc::channel(1);
        let (mut blocking_tx, blocking_rx) = futures::channel::mpsc::channel(1);
        let rxs = Arc::new([Mutex::new(Some(async_rx)), Mutex::new(Some(blocking_rx))]);

        let _handle = crate::add_hyper_fixture(PATH, {
            let rxs = rxs.clone();
            move |req| {
                let rxs = rxs.clone();
                async move {
                    let is_blocking = req.is_blocking() as usize;
                    let rx = rxs[is_blocking].lock().unwrap().take().unwrap();

                    let body = http_body_util::StreamBody::new(rx).boxed();
                    let res = Response::new(body);

                    (res, Ok(()))
                }
            }
        });
        #[cfg(feature = "blocking")]
        {
            let builder = crate::init_builder_blocking()
                .unwrap()
                .no_caching()
                .max_response_buffer_size(1);
            let client = builder.build_blocking().unwrap();
            // Workaround for NSURLSession buffering the first 512 bytes
            // https://developer.apple.com/forums/thread/64875
            blocking_tx
                .try_send(Ok(Frame::data(Bytes::from_static(&[0; 512]))))
                .unwrap();
            let res = client.request(NyquestRequest::get(PATH)).unwrap();
            let mut read = res.into_read();
            read.read_exact(&mut [0; 512]).unwrap();
            blocking_tx
                .try_send(Ok(Frame::data(Bytes::from_static(b"1"))))
                .unwrap();
            let mut buf = [0; 16];
            assert_eq!((read.read(&mut buf).unwrap(), buf[0]), (1, b'1'));
            blocking_tx
                .try_send(Ok(Frame::data(Bytes::from_static(b"2"))))
                .unwrap();
            assert_eq!((read.read(&mut buf).unwrap(), buf[0]), (1, b'2'));
            blocking_tx
                .try_send(Ok(Frame::data(Bytes::from_static(b"3"))))
                .unwrap();
            assert_eq!((read.read(&mut buf).unwrap(), buf[0]), (1, b'3'));
            drop(blocking_tx);
            assert_eq!(read.read(&mut buf).unwrap(), 0);
        }
        #[cfg(feature = "async")]
        {
            TOKIO_RT.block_on(async {
                let builder = crate::init_builder()
                    .await
                    .unwrap()
                    .no_caching()
                    .max_response_buffer_size(1);
                let client = builder.build_async().await.unwrap();
                async_tx
                    .try_send(Ok(Frame::data(Bytes::from_static(&[0; 512]))))
                    .unwrap();
                let res = client.request(NyquestRequest::get(PATH)).await.unwrap();
                let mut read = res.into_async_read();
                read.read_exact(&mut [0; 512]).await.unwrap();
                async_tx
                    .try_send(Ok(Frame::data(Bytes::from_static(b"1"))))
                    .unwrap();
                let mut buf = [0; 16];
                assert_eq!((read.read(&mut buf).await.unwrap(), buf[0]), (1, b'1'));
                async_tx
                    .try_send(Ok(Frame::data(Bytes::from_static(b"2"))))
                    .unwrap();
                assert_eq!((read.read(&mut buf).await.unwrap(), buf[0]), (1, b'2'));
                async_tx
                    .try_send(Ok(Frame::data(Bytes::from_static(b"3"))))
                    .unwrap();
                assert_eq!((read.read(&mut buf).await.unwrap(), buf[0]), (1, b'3'));
                drop(async_tx);
                assert_eq!(read.read(&mut buf).await.unwrap(), 0);
            });
        }
    }
}
