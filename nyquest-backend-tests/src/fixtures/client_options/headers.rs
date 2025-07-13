#[cfg(test)]
mod tests {
    use http_body_util::Full;
    use hyper::header::{ACCEPT, CONTENT_LANGUAGE, USER_AGENT};
    #[cfg(feature = "blocking")]
    use nyquest::blocking::Body as NyquestBlockingBody;
    #[cfg(feature = "async")]
    use nyquest::r#async::Body as NyquestAsyncBody;
    use nyquest::Request as NyquestRequest;

    use crate::*;

    #[test]
    fn test_user_agent() {
        const PATH: &str = "client_options/user_agent";
        const USER_AGENT_VALUE: &str = "Nyquest/1.0 (Test User Agent)";
        let _handle = crate::add_hyper_fixture(PATH, {
            move |req| async move {
                let user_agent = req
                    .headers()
                    .get(USER_AGENT)
                    .map(|v| v.to_str().unwrap().to_owned());
                let user_agent = Bytes::from(user_agent.unwrap_or_default().into_bytes());

                let res = Response::new(Full::new(user_agent));
                (res, Ok(()))
            }
        });

        let assertions = |user_agent: String| {
            assert_eq!(user_agent, USER_AGENT_VALUE);
        };

        #[cfg(feature = "blocking")]
        {
            let builder = crate::init_builder_blocking()
                .unwrap()
                .user_agent(USER_AGENT_VALUE);
            let client = builder.build_blocking().unwrap();
            let res = client
                .request(NyquestRequest::get(PATH))
                .unwrap()
                .text()
                .unwrap();
            assertions(res);
        }

        #[cfg(feature = "async")]
        {
            let res = TOKIO_RT.block_on(async {
                let builder = crate::init_builder()
                    .await
                    .unwrap()
                    .user_agent(USER_AGENT_VALUE);
                let client = builder.build_async().await.unwrap();
                client
                    .request(NyquestRequest::get(PATH))
                    .await
                    .unwrap()
                    .text()
                    .await
                    .unwrap()
            });
            assertions(res);
        }
    }

    #[test]
    fn test_default_headers() {
        const PATH: &str = "client_options/default_headers";
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
            let builder = crate::init_builder_blocking()
                .unwrap()
                .with_header("Accept", ACCEPT_VALUE)
                .with_header("Content-Language", CONTENT_LANGUAGE_VALUE);
            let client = builder.build_blocking().unwrap();
            let res = client
                .request(
                    NyquestRequest::post(PATH).with_body(NyquestBlockingBody::plain_text("aa")),
                )
                .unwrap()
                .text()
                .unwrap();
            assertions(res);
        }

        #[cfg(feature = "async")]
        {
            let res = TOKIO_RT.block_on(async {
                let builder = crate::init_builder()
                    .await
                    .unwrap()
                    .with_header("Accept", ACCEPT_VALUE)
                    .with_header("Content-Language", CONTENT_LANGUAGE_VALUE);
                let client = builder.build_async().await.unwrap();
                client
                    .request(
                        NyquestRequest::post(PATH).with_body(NyquestAsyncBody::plain_text("aa")),
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
}
