#![cfg(any(feature = "async", feature = "blocking"))]

use std::fmt;

use nyquest::ClientBuilder;
use nyquest_interface::client::ClientOptions;

#[derive(Clone, Copy)]
struct TestBackend(&'static str);

#[cfg(feature = "async")]
mod async_tests {
    use std::pin::Pin;
    #[cfg(feature = "async-stream")]
    use std::task::{Context, Poll};

    use futures::executor::block_on;
    use nyquest_interface::r#async::{AsyncBackend, AsyncClient, AsyncResponse, Request};

    use super::*;

    #[derive(Clone)]
    pub struct TestClient(&'static str);

    pub struct TestResponse {
        body: Vec<u8>,
        #[cfg(feature = "async-stream")]
        position: usize,
    }

    impl AsyncBackend for TestBackend {
        type AsyncClient = TestClient;

        async fn create_async_client(
            &self,
            _options: ClientOptions,
        ) -> nyquest_interface::Result<Self::AsyncClient> {
            Ok(TestClient(self.0))
        }
    }

    impl AsyncClient for TestClient {
        type Response = TestResponse;

        async fn request(&self, _request: Request) -> nyquest_interface::Result<Self::Response> {
            Ok(TestResponse {
                body: self.0.as_bytes().to_vec(),
                #[cfg(feature = "async-stream")]
                position: 0,
            })
        }
    }

    impl AsyncResponse for TestResponse {
        fn status(&self) -> u16 {
            200
        }

        fn content_length(&self) -> Option<u64> {
            Some(self.body.len() as u64)
        }

        fn get_header(&self, _header: &str) -> nyquest_interface::Result<Vec<String>> {
            Ok(Vec::new())
        }

        async fn text(self: Pin<&mut Self>) -> nyquest_interface::Result<String> {
            Ok(String::from_utf8(self.get_mut().body.clone()).unwrap())
        }

        async fn bytes(self: Pin<&mut Self>) -> nyquest_interface::Result<Vec<u8>> {
            Ok(self.get_mut().body.clone())
        }
    }

    #[cfg(feature = "async-stream")]
    impl nyquest_interface::r#async::futures_io::AsyncRead for TestResponse {
        fn poll_read(
            mut self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            buffer: &mut [u8],
        ) -> Poll<std::io::Result<usize>> {
            let remaining = &self.body[self.position..];
            let count = remaining.len().min(buffer.len());
            buffer[..count].copy_from_slice(&remaining[..count]);
            self.position += count;
            Poll::Ready(Ok(count))
        }
    }

    #[test]
    fn async_clients_can_use_different_backends_without_global_registration() {
        block_on(async {
            let first_backend = TestBackend("first");
            let second_backend = TestBackend("second");
            let first = ClientBuilder::default()
                .custom_backend(&first_backend)
                .build_async()
                .await
                .unwrap();
            let second = ClientBuilder::default()
                .custom_backend(&second_backend)
                .build_async()
                .await
                .unwrap();

            assert_eq!(
                first
                    .request(nyquest::r#async::Request::get("https://example.com"))
                    .await
                    .unwrap()
                    .text()
                    .await
                    .unwrap(),
                "first"
            );
            assert_eq!(
                second
                    .request(nyquest::r#async::Request::get("https://example.com"))
                    .await
                    .unwrap()
                    .text()
                    .await
                    .unwrap(),
                "second"
            );
        });
    }
}

#[cfg(feature = "blocking")]
mod blocking_tests {
    use std::io::Cursor;
    #[cfg(feature = "blocking-stream")]
    use std::io::Read;

    use nyquest_interface::blocking::{BlockingBackend, BlockingClient, BlockingResponse, Request};

    use super::*;

    #[derive(Clone)]
    pub struct TestClient(&'static str);

    pub struct TestResponse(Cursor<Vec<u8>>);

    impl BlockingBackend for TestBackend {
        type BlockingClient = TestClient;

        fn create_blocking_client(
            &self,
            _options: ClientOptions,
        ) -> nyquest_interface::Result<Self::BlockingClient> {
            Ok(TestClient(self.0))
        }
    }

    impl BlockingClient for TestClient {
        type Response = TestResponse;

        fn request(&self, _request: Request) -> nyquest_interface::Result<Self::Response> {
            Ok(TestResponse(Cursor::new(self.0.as_bytes().to_vec())))
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
            Ok(Vec::new())
        }

        fn text(&mut self) -> nyquest_interface::Result<String> {
            Ok(String::from_utf8(self.0.get_ref().clone()).unwrap())
        }

        fn bytes(&mut self) -> nyquest_interface::Result<Vec<u8>> {
            Ok(self.0.get_ref().clone())
        }
    }

    #[cfg(feature = "blocking-stream")]
    impl Read for TestResponse {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            self.0.read(buffer)
        }
    }

    #[test]
    fn blocking_clients_can_use_different_backends_without_global_registration() {
        let first_backend = TestBackend("first");
        let second_backend = TestBackend("second");
        let first = ClientBuilder::default()
            .custom_backend(&first_backend)
            .build_blocking()
            .unwrap();
        let second = ClientBuilder::default()
            .custom_backend(&second_backend)
            .build_blocking()
            .unwrap();

        assert_eq!(
            first
                .request(nyquest::blocking::Request::get("https://example.com"))
                .unwrap()
                .text()
                .unwrap(),
            "first"
        );
        assert_eq!(
            second
                .request(nyquest::blocking::Request::get("https://example.com"))
                .unwrap()
                .text()
                .unwrap(),
            "second"
        );
    }
}

impl fmt::Debug for TestBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("TestBackend").field(&self.0).finish()
    }
}
