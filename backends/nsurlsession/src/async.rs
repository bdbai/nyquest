use std::future::poll_fn;
use std::pin::Pin;
use std::sync::Arc;
use std::task::Poll;

use nyquest_interface::client::ClientOptions;
use nyquest_interface::r#async::{AsyncBackend, AsyncClient, AsyncResponse};
use nyquest_interface::Result as NyquestResult;
use objc2::runtime::ProtocolObject;

pub(crate) mod waker;

use crate::client::NSUrlSessionClient;
use crate::datatask::{DataTaskDelegate, GenericWaker};
use crate::error::IntoNyquestResult;
use crate::r#async::waker::AsyncWaker;
use crate::response::NSUrlSessionResponse;
use crate::NSUrlSessionBackend;

#[derive(Clone)]
pub struct NSUrlSessionAsyncClient {
    inner: NSUrlSessionClient,
}

pub struct NSUrlSessionAsyncResponse {
    inner: NSUrlSessionResponse,
    max_response_buffer_size: u64,
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

    async fn text(mut self: Pin<&mut Self>) -> NyquestResult<String> {
        let bytes = self.as_mut().bytes().await?;
        self.inner.convert_bytes_to_string(bytes)
    }

    async fn bytes(mut self: Pin<&mut Self>) -> NyquestResult<Vec<u8>> {
        self.inner
            .shared
            .set_max_response_buffer_size(self.max_response_buffer_size);
        let inner = &mut self.inner;
        let inner_waker = coerce_waker(inner.shared.waker_ref());
        unsafe {
            inner.task.resume();
        }
        poll_fn(|cx| {
            if inner.shared.is_completed() {
                return Poll::Ready(());
            }
            inner_waker.register(cx);
            Poll::Pending
        })
        .await;
        let res = inner.shared.take_response_buffer()?;
        unsafe {
            inner.task.error().into_nyquest_result()?;
        }
        Ok(res)
    }
}

#[cfg(feature = "async-stream")]
impl nyquest_interface::r#async::futures_io::AsyncRead for NSUrlSessionAsyncResponse {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut [u8],
    ) -> Poll<std::io::Result<usize>> {
        let inner = &mut self.inner;

        if let Some(result) = inner.consume_response_to_buffer(buf) {
            return Poll::Ready(result);
        }

        let inner_waker = coerce_waker(inner.shared.waker_ref());
        inner_waker.register(cx);
        unsafe {
            inner.task.resume();
        }
        Poll::Pending
    }
}

impl AsyncClient for NSUrlSessionAsyncClient {
    type Response = NSUrlSessionAsyncResponse;

    async fn request(
        &self,
        req: nyquest_interface::r#async::Request,
    ) -> NyquestResult<Self::Response> {
        let waker = GenericWaker::Async(Arc::new(AsyncWaker::new()));
        let (task, mut writer) = self.inner.build_data_task(req, &waker, |s| {
            #[cfg(feature = "async-stream")]
            {
                use nyquest_interface::r#async::BoxedStream;
                match &s {
                    BoxedStream::Sized { content_length, .. } => Some(*content_length),
                    BoxedStream::Unsized { .. } => None,
                }
            }
            #[cfg(not(feature = "async-stream"))]
            {
                let _ = s;
                unreachable!("async-stream feature is disabled")
            }
        })?;
        let shared = unsafe {
            let delegate = DataTaskDelegate::new(waker, self.inner.allow_redirects);
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
            #[cfg(feature = "async-stream")]
            if let Some(writer) = &mut writer {
                use futures_util::AsyncRead;
                writer.poll_progress(|stream, buf| Pin::new(stream).poll_read(cx, buf))?;
            }
            #[cfg(not(feature = "async-stream"))]
            {
                let _ = &mut writer;
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
            max_response_buffer_size: self.inner.max_response_buffer_size,
        })
    }
}

impl AsyncBackend for NSUrlSessionBackend {
    type AsyncClient = NSUrlSessionAsyncClient;

    async fn create_async_client(
        &self,
        options: ClientOptions,
    ) -> NyquestResult<Self::AsyncClient> {
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
