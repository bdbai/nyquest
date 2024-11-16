#[cfg(test)]
mod tests {
    use hyper::Method;
    use nyquest::Request;

    use crate::*;

    #[test]
    fn test_get_json() {
        let _handle = crate::add_hyper_fixture("/blocking_get_json", |req| async move {
            let body = r#"{"message": "Hello, world!"}"#;
            let res = Response::new(Full::new(Bytes::from(body)));
            (res, (req.method() == Method::GET).then_some(()).ok_or(req))
        });
        let builder = crate::init_builder_blocking().unwrap();
        let assertions = |status, content| {
            assert_eq!(status, 200);
            assert_eq!(content, r#"{"message": "Hello, world!"}"#);
        };
        #[cfg(feature = "blocking")]
        {
            let client = builder.clone().build_blocking().unwrap();
            let res = client.request(Request::get("blocking_get_json")).unwrap();
            let status = res.status();
            let content = res.text().unwrap();
            assertions(status, content);
        }
        #[cfg(feature = "async")]
        {
            let (status, content) = TOKIO_RT.block_on(async {
                let client = builder.build_async().await.unwrap();
                let res = client
                    .request(Request::get("blocking_get_json"))
                    .await
                    .unwrap();
                let status = res.status();
                (status, res.text().await.unwrap())
            });
            assertions(status, content);
        }
    }
}
