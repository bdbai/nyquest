#[cfg(test)]
mod tests {
    use futures::stream;
    use http_body_util::BodyExt;
    use hyper::{Method, Response};
    use nyquest::Request as NyquestRequest;

    use crate::*;

    #[test]
    fn test_chunked_encoding() {
        const PATH: &str = "scenarios/chunked_encoding";
        const CHUNKS: [&str; 4] = ["Hello", ", ", "chunked ", "world!"];

        let _handle = crate::add_hyper_fixture(PATH, {
            move |req| {
                async move {
                    // Create a streaming response body by yielding each chunk with a small delay
                    let stream = stream::iter(CHUNKS.iter().map(|chunk| {
                        let chunk = Bytes::copy_from_slice(chunk.as_bytes());
                        Ok::<_, hyper::Error>(hyper::body::Frame::data(chunk))
                    }));

                    // Convert the stream to a boxed body
                    let body = http_body_util::StreamBody::new(stream).boxed();
                    let res = Response::new(body);

                    (res, (req.method() == Method::GET).then_some(()).ok_or(req))
                }
            }
        });

        let expected_content = CHUNKS.concat();
        let builder = crate::init_builder_blocking().unwrap();

        let assertions = |content: &str| {
            assert_eq!(content, expected_content);
        };

        #[cfg(feature = "blocking")]
        {
            let client = builder.clone().build_blocking().unwrap();
            let res = client.request(NyquestRequest::get(PATH)).unwrap();
            let content = res.text().unwrap();
            assertions(&content);
        }

        #[cfg(feature = "async")]
        {
            let content = TOKIO_RT.block_on(async {
                let client = builder.build_async().await.unwrap();
                let res = client.request(NyquestRequest::get(PATH)).await.unwrap();
                res.text().await.unwrap()
            });
            assertions(&content);
        }
    }
}
