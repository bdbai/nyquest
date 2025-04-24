#[cfg(test)]
mod tests {
    use std::{
        ops::Deref,
        sync::{Arc, OnceLock},
    };

    use form_urlencoded::Target;
    use http_body_util::BodyExt;
    use hyper::{header::CONTENT_TYPE, Method, StatusCode};
    use memchr::memmem;
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
    fn test_body_form() {
        const PATH: &str = "requests/body_form";
        const VALUE1: &str = "valu e1";
        const VALUE2: &str = "value=2哈";
        const VALUE3: &str = "val&&u e +3";
        let received_body = Arc::new([const { OnceLock::new() }; 2]);
        let _handle = crate::add_hyper_fixture(PATH, {
            let received_body = Arc::clone(&received_body);
            move |req| {
                let received_body = Arc::clone(&received_body);
                async move {
                    let is_blocking =
                        req.headers().get("blocking").map(|v| v.as_bytes()) == Some(b"1");
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
            let mut form = form_urlencoded::parse(&bytes);
            assert_eq!(double_deref(&form.next()), Some(("key1", VALUE1)));
            assert_eq!(double_deref(&form.next()), Some(("key2", VALUE2)));
            assert_eq!(double_deref(&form.next()), Some(("key3", VALUE3)));
            assert_eq!(form.next().as_ref().map(|kv| &*kv.0), Some("key 4"));
            assert!(form.next().is_none());
            assert!(memmem::find(&bytes, b"valu+e1").is_some());
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
            let client = builder.clone().build_blocking().unwrap();
            client
                .request(NyquestRequest::post(PATH).with_body(req_body))
                .unwrap();
            assertions(&*received_body[1].get().unwrap());
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
            assertions(&*received_body[0].get().unwrap());
        }
    }
}
