#[cfg(test)]
mod tests {
    use std::{
        ops::Deref,
        sync::{Arc, OnceLock},
    };

    use form_urlencoded::Target;
    use http_body_util::BodyExt;
    use hyper::{Method, StatusCode};
    #[cfg(feature = "blocking")]
    use nyquest::blocking::Body as NyquestBlockingBody;
    #[cfg(feature = "async")]
    use nyquest::r#async::Body as NyquestAsyncBody;
    use nyquest::{body_form, Request as NyquestRequest};

    use crate::*;

    fn double_deref<'a, A: ?Sized, B: ?Sized>(
        t: &'a Option<(impl Deref<Target = A>, impl Deref<Target = B>)>,
    ) -> Option<(&'a A, &'a B)> {
        t.as_ref().map(|(a, b)| (&**a, &**b))
    }
    #[test]
    fn test_body_query() {
        const PATH: &str = "responses/get_text";
        const VALUE1: &str = "valu e1";
        const VALUE2: &str = "value=2";
        const VALUE3: &str = "val&&u e +3";
        let mut received_body = Arc::new(OnceLock::default());
        let _handle = crate::add_hyper_fixture(PATH, {
            let received_body = Arc::clone(&received_body);
            move |req| {
                let received_body = Arc::clone(&received_body);
                async move {
                    received_body
                        .set(req.into_body().collect().await.unwrap().to_bytes())
                        .ok();
                    let res = Response::new(Full::new(Default::default()));
                    (res, Ok(()))
                }
            }
        });
        let builder = crate::init_builder_blocking().unwrap();
        let assertions = |(_status, _content_len, _content)| {
            let mut form = form_urlencoded::parse(received_body.get().unwrap());
            assert_eq!(double_deref(&form.next()), Some(("key1", VALUE1)));
            assert_eq!(double_deref(&form.next()), Some(("key2", VALUE2)));
            assert_eq!(double_deref(&form.next()), Some(("key3", VALUE3)));
            assert!(form.next().is_none());
        };
        #[cfg(feature = "blocking")]
        {
            let req_body = body_form! {
                "key1" => VALUE1,
                "key2" => VALUE2,
                "key3" => VALUE3,
            };
            let client = builder.clone().build_blocking().unwrap();
            let res = client
                .request(NyquestRequest::post(PATH).with_body(req_body))
                .unwrap();
            let status = res.status();
            let content_len = res.content_length();
            let content = res.text().unwrap();
            assertions((status, content_len, content));
        }
        #[cfg(feature = "async")]
        {
            let req_body = body_form! {
                "key1" => VALUE1,
                "key2" => VALUE2,
                "key3" => VALUE3,
            };
            let facts = TOKIO_RT.block_on(async {
                let client = builder.build_async().await.unwrap();
                let res = client
                    .request(NyquestRequest::post(PATH).with_body(req_body))
                    .await
                    .unwrap();
                let status = res.status();
                let content_len = res.content_length();
                (status, content_len, res.text().await.unwrap())
            });
            assertions(facts);
        }
    }
}
