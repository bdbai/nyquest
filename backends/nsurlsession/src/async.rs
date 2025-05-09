use std::future::poll_fn;
use std::pin::Pin;
use std::task::{Context, Poll};

use nyquest_interface::client::{BuildClientResult, ClientOptions};
use nyquest_interface::r#async::{futures_io, AsyncBackend, AsyncClient, AsyncResponse};
use nyquest_interface::{Error as NyquestError, Result as NyquestResult};
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

    async fn text(mut self: Pin<&mut Self>) -> NyquestResult<String> {
        let bytes = self.as_mut().bytes().await?;
        self.inner.convert_bytes_to_string(bytes)
    }

    async fn bytes(mut self: Pin<&mut Self>) -> NyquestResult<Vec<u8>> {
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

impl futures_io::AsyncRead for NSUrlSessionAsyncResponse {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<futures_io::Result<usize>> {
        let inner = &mut self.inner;

        let read_len = inner.shared.with_response_buffer_mut(|data| {
            let read_len = if data.len() > buf.len() {
                unsafe {
                    inner.task.suspend();
                }
                buf.len()
            } else {
                data.len()
            };
            buf[..read_len].copy_from_slice(&data[..read_len]);
            data.drain(..read_len);
            read_len
        });
        match read_len {
            Ok(read_len @ 1..) => {
                return Poll::Ready(Ok(read_len));
            }
            Err(NyquestError::Io(e)) => return Poll::Ready(Err(e)),
            Err(e) => unreachable!("Unexpected error: {e}"),
            Ok(0) if inner.shared.is_completed() => return Poll::Ready(Ok(0)),
            Ok(0) => {}
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
        let task = self.inner.build_data_task(req)?;
        let shared = unsafe {
            let delegate = DataTaskDelegate::new(
                GenericWaker::Async(AsyncWaker::new()),
                self.inner.max_response_buffer_size,
                self.inner.allow_redirects,
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
