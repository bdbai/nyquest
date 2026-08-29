#[cfg(test)]
mod tests {

    use http_body_util::Full;
    use nyquest::Request as NyquestRequest;

    use crate::*;

    #[derive(Clone, Copy)]
    struct TestBackend {
        body: &'static str,
        expected_path: &'static str,
    }

    #[derive(Clone)]
    pub struct TestClient(TestBackend);

    fn assert_request<T>(request: &nyquest_interface::Request<T>, expected_path: &str) {
        let uri = &*request.relative_uri;
        assert_eq!(uri, expected_path);
    }

    mod blocking {
        use std::io::Cursor;
        #[cfg(feature = "blocking-stream")]
        use std::io::Read;

        use nyquest_interface::{
            blocking::{BlockingBackend, BlockingClient, BlockingResponse, Request},
            client::ClientOptions,
        };

        use super::*;

        pub struct TestResponse(Cursor<&'static [u8]>);

        impl BlockingBackend for TestBackend {
            type BlockingClient = TestClient;

            fn create_blocking_client(
                &self,
                _options: ClientOptions,
            ) -> nyquest_interface::Result<Self::BlockingClient> {
                Ok(TestClient(self.clone()))
            }
        }

        impl BlockingClient for TestClient {
            type Response = TestResponse;

            fn request(&self, request: Request) -> nyquest_interface::Result<Self::Response> {
                assert_request(&request, self.0.expected_path);
                Ok(TestResponse(Cursor::new(self.0.body.as_bytes())))
            }
        }

        impl BlockingResponse for TestResponse {
            fn status(&self) -> u16 {
                200
            }

            fn content_length(&self) -> Option<u64> {
                Some(self.0.get_ref().len() as u64)
            }

            fn get_header(&self, _header: &str) -> nyquest_interface::Result<Vec<String>> {
                Ok(vec![])
            }

            fn text(&mut self) -> nyquest_interface::Result<String> {
                Ok(String::from_utf8(self.0.get_ref().to_vec()).unwrap())
            }

            fn bytes(&mut self) -> nyquest_interface::Result<Vec<u8>> {
                Ok(self.0.get_ref().to_vec())
            }
        }

        #[cfg(feature = "blocking-stream")]
        impl Read for TestResponse {
            fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
                self.0.read(buffer)
            }
        }
    }

    mod r#async {
        use std::pin::Pin;
        use std::task::{Context, Poll};

        use futures::io::Cursor;
        use futures::AsyncRead;
        use nyquest_interface::{
            client::ClientOptions,
            r#async::{AsyncBackend, AsyncClient, AsyncResponse, Request},
        };

        use super::*;

        pub struct TestResponse(Cursor<&'static [u8]>);

        impl AsyncBackend for TestBackend {
            type AsyncClient = TestClient;

            async fn create_async_client(
                &self,
                _options: ClientOptions,
            ) -> nyquest_interface::Result<Self::AsyncClient> {
                Ok(TestClient(self.clone()))
            }
        }

        impl AsyncClient for TestClient {
            type Response = TestResponse;

            async fn request(&self, request: Request) -> nyquest_interface::Result<Self::Response> {
                assert_request(&request, self.0.expected_path);
                let response = TestResponse(Cursor::new(self.0.body.as_bytes()));
                Ok(response)
            }
        }

        impl AsyncResponse for TestResponse {
            fn status(&self) -> u16 {
                200
            }

            fn content_length(&self) -> Option<u64> {
                Some(self.0.get_ref().len() as u64)
            }

            fn get_header(&self, _header: &str) -> nyquest_interface::Result<Vec<String>> {
                Ok(vec![])
            }

            async fn text(self: Pin<&mut Self>) -> nyquest_interface::Result<String> {
                Ok(String::from_utf8(self.0.get_ref().to_vec()).unwrap())
            }

            async fn bytes(self: Pin<&mut Self>) -> nyquest_interface::Result<Vec<u8>> {
                Ok(self.0.get_ref().to_vec())
            }
        }

        impl AsyncRead for TestResponse {
            fn poll_read(
                mut self: Pin<&mut Self>,
                cx: &mut Context<'_>,
                buf: &mut [u8],
            ) -> Poll<io::Result<usize>> {
                Pin::new(&mut self.0).poll_read(cx, buf)
            }
        }
    }

    #[test]
    fn test_response_within_limit() {
        const PATH: &str = "client_options/custom_backend";
        const BODY: &str = "custom body";

        let _handle = crate::add_hyper_fixture(PATH, |_| async {
            (
                Response::new(Full::new(Bytes::from_static(b"server"))),
                Ok(()),
            )
        });

        let assertions = |content: String| {
            assert_eq!(content, BODY);
        };

        let backend = TestBackend {
            body: BODY,
            expected_path: PATH,
        };

        #[cfg(feature = "blocking")]
        {
            let builder = crate::init_builder_blocking()
                .unwrap()
                .custom_backend(&backend);
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
                    .custom_backend(&backend);
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
