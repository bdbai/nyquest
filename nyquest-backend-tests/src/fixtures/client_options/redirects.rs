#[cfg(test)]
mod tests {
    use http_body_util::Full;
    use nyquest::Request as NyquestRequest;

    use crate::*;

    async fn redirect_handler(req: Request<body::Incoming>) -> FixtureAssertionResult {
        let is_redirected = req
            .uri()
            .query()
            .unwrap_or_default()
            .contains("redirected=1");

        let res = if is_redirected {
            Response::new(Full::new(Bytes::from("redirected")))
        } else {
            Response::builder()
                .header("Location", "?redirected=1")
                .status(302)
                .body(Full::new(Bytes::from("initial")))
                .unwrap()
        };
        (res.into(), Ok(()))
    }

    #[test]
    fn test_follow_redirects() {
        const PATH: &str = "client_options/follow_redirects";
        let _handle = crate::add_hyper_fixture(PATH, redirect_handler);

        let assertions = |status: u16, body: String| {
            assert_eq!(status, 200);
            assert_eq!(body, "redirected");
        };

        #[cfg(feature = "blocking")]
        {
            let builder = crate::init_builder_blocking().unwrap();
            let client = builder.build_blocking().unwrap();
            let res = client.request(NyquestRequest::get(PATH)).unwrap();
            assertions(res.status().into(), res.text().unwrap());
        }

        #[cfg(feature = "async")]
        {
            let (status, body) = TOKIO_RT.block_on(async {
                let builder = crate::init_builder().await.unwrap();
                let client = builder.build_async().await.unwrap();
                let res = client.request(NyquestRequest::get(PATH)).await.unwrap();
                (res.status().into(), res.text().await.unwrap())
            });
            assertions(status, body);
        }
    }

    #[test]
    fn test_no_redirects() {
        const PATH: &str = "client_options/no_redirects";

        let _handle = crate::add_hyper_fixture(PATH, redirect_handler);

        let assertions = |status: u16, body: String| {
            assert_eq!(status, 302);
            assert_eq!(body, "initial");
        };

        #[cfg(feature = "blocking")]
        {
            let builder = crate::init_builder_blocking().unwrap().no_redirects();
            let client = builder.build_blocking().unwrap();
            let res = client.request(NyquestRequest::get(PATH)).unwrap();
            assertions(res.status().into(), res.text().unwrap());
        }

        #[cfg(feature = "async")]
        {
            let (status, body) = TOKIO_RT.block_on(async {
                let builder = crate::init_builder().await.unwrap().no_redirects();
                let client = builder.build_async().await.unwrap();
                let res = client.request(NyquestRequest::get(PATH)).await.unwrap();
                (res.status().into(), res.text().await.unwrap())
            });
            assertions(status, body);
        }
    }
}
