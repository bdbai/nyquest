#[cfg(test)]
mod tests {
    use http_body_util::Full;
    use hyper::header::USER_AGENT;
    use nyquest::Request as NyquestRequest;

    use crate::*;

    #[test]
    fn test_user_agent() {
        const PATH: &str = "client_options/user_agent";
        const USER_AGENT_VALUE: &str = "Nyquest/1.0 (Test User Agent)";
        let _handle = crate::add_hyper_fixture(PATH, {
            move |req: Request<body::Incoming>| async move {
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
}
