use std::future::poll_fn;
use std::task::Poll;

use nyquest_interface::client::{BuildClientResult, ClientOptions};
use nyquest_interface::r#async::{AsyncBackend, AsyncClient, AsyncResponse};
use nyquest_interface::Result as NyquestResult;
use objc2::runtime::ProtocolObject;
use waker::AsyncWaker;

pub(crate) mod waker;

use crate::client::NSUrlSessionClient;
use crate::datatask::{DataTaskDelegate, GenericWaker};
use crate::error::IntoNyquestResult;
use crate::response::NSUrlSessionResponse;
use crate::NSUrlSessionBackend;

#[derive(Clone)]
pub struct NSUrlSessionAsyncClient {
    inner: NSUrlSessionClient,
}

pub struct NSUrlSessionAsyncResponse {
    inner: NSUrlSessionResponse,
}

impl AsyncResponse for NSUrlSessionAsyncResponse {
    fn status(&self) -> u16 {
        self.inner.status()
    }

    fn content_length(&self) -> Option<u64> {
        self.inner.content_length()
    }

    fn get_header(&self, header: &str) -> NyquestResult<Vec<String>> {
        self.inner.get_header(header)
    }

    async fn text(&mut self) -> NyquestResult<String> {
        let bytes = self.bytes().await?;
        self.inner.convert_bytes_to_string(bytes)
    }

    async fn bytes(&mut self) -> NyquestResult<Vec<u8>> {
        let inner_waker = coerce_waker(self.inner.shared.waker_ref());
        unsafe {
            self.inner.task.resume();
        }
        poll_fn(|cx| {
            if self.inner.shared.is_completed() {
                return Poll::Ready(());
            }
            inner_waker.register(cx);
            Poll::Pending
        })
        .await;
        unsafe {
            self.inner.task.error().into_nyquest_result()?;
        }
        self.inner.shared.take_response_buffer()
    }
}

impl AsyncClient for NSUrlSessionAsyncClient {
    type Response = NSUrlSessionAsyncResponse;

    async fn request(
        &self,
        req: nyquest_interface::r#async::Request,
    ) -> NyquestResult<Self::Response> {
        let task = self.inner.build_data_task(req)?;
        let shared = unsafe {
            let delegate = DataTaskDelegate::new(
                GenericWaker::Async(AsyncWaker::new()),
                self.inner.max_response_buffer_size,
            );
            task.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
            task.resume();
            DataTaskDelegate::into_shared(delegate)
        };
        let inner_waker = coerce_waker(shared.waker_ref());
        // TODO: cancellation
        let response = poll_fn(|cx| {
            if let Some(response) = shared.try_take_response().into_nyquest_result().transpose() {
                return Poll::Ready(response);
            }
            inner_waker.register(cx);
            Poll::Pending
        })
        .await?;
        unsafe {
            task.error().into_nyquest_result()?;
        }
        Ok(NSUrlSessionAsyncResponse {
            inner: NSUrlSessionResponse {
                task,
                response,
                shared,
            },
        })
    }
}

impl AsyncBackend for NSUrlSessionBackend {
    type AsyncClient = NSUrlSessionAsyncClient;

    async fn create_async_client(
        &self,
        options: ClientOptions,
    ) -> BuildClientResult<Self::AsyncClient> {
        Ok(NSUrlSessionAsyncClient {
            inner: NSUrlSessionClient::create(options)?,
        })
    }
}

#[allow(irrefutable_let_patterns)]
fn coerce_waker(waker: &GenericWaker) -> &AsyncWaker {
    if let GenericWaker::Async(waker) = waker {
        waker
    } else {
        unreachable!("should not be called in async context")
    }
}
