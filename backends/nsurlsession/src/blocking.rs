use nyquest_interface::blocking::{BlockingBackend, BlockingClient, BlockingResponse, Request};
use nyquest_interface::client::ClientOptions;
use nyquest_interface::{Error as NyquestError, Result as NyquestResult};
use objc2::runtime::ProtocolObject;
use waker::BlockingWaker;

pub(crate) mod waker;

use crate::client::NSUrlSessionClient;
use crate::datatask::{DataTaskDelegate, GenericWaker};
use crate::error::IntoNyquestResult;
use crate::response::NSUrlSessionResponse;
use crate::NSUrlSessionBackend;

#[derive(Clone)]
pub struct NSUrlSessionBlockingClient {
    inner: NSUrlSessionClient,
}
pub struct NSUrlSessionBlockingResponse {
    inner: NSUrlSessionResponse,
    max_response_buffer_size: u64,
}

impl std::io::Read for NSUrlSessionBlockingResponse {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let inner = &mut self.inner;

        loop {
            let read_len = inner.shared.with_response_buffer_for_stream_mut(|data| {
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
                    return Ok(read_len);
                }
                Err(NyquestError::Io(e)) => return Err(e),
                Err(e) => unreachable!("Unexpected error: {e}"),
                Ok(0) if inner.shared.is_completed() => return Ok(0),
                Ok(0) => {}
            }

            let inner_waker = coerce_waker(inner.shared.waker_ref());
            inner_waker.register_current_thread();
            unsafe {
                inner.task.resume();
            }
            std::thread::park();
        }
    }
}

impl BlockingResponse for NSUrlSessionBlockingResponse {
    fn status(&self) -> u16 {
        self.inner.status()
    }

    fn content_length(&self) -> Option<u64> {
        self.inner.content_length()
    }

    fn get_header(&self, header: &str) -> nyquest_interface::Result<Vec<String>> {
        self.inner.get_header(header)
    }

    fn text(&mut self) -> nyquest_interface::Result<String> {
        let bytes = self.bytes()?;
        self.inner.convert_bytes_to_string(bytes)
    }

    fn bytes(&mut self) -> nyquest_interface::Result<Vec<u8>> {
        self.inner
            .shared
            .set_max_response_buffer_size(self.max_response_buffer_size);
        let inner_waker = coerce_waker(self.inner.shared.waker_ref());
        unsafe {
            self.inner.task.resume();
        }
        inner_waker.register_current_thread();

        while !self.inner.shared.is_completed() {
            std::thread::park();
        }
        let res = self.inner.shared.take_response_buffer()?;
        unsafe {
            self.inner.task.error().into_nyquest_result()?;
        }
        Ok(res)
    }
}

impl BlockingClient for NSUrlSessionBlockingClient {
    type Response = NSUrlSessionBlockingResponse;

    fn request(&self, req: Request) -> nyquest_interface::Result<Self::Response> {
        let task = self.inner.build_data_task(req)?;
        let shared = unsafe {
            let delegate = DataTaskDelegate::new(
                GenericWaker::Blocking(BlockingWaker::new_from_current_thread()),
                self.inner.allow_redirects,
            );
            task.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
            task.resume();
            DataTaskDelegate::into_shared(delegate)
        };
        loop {
            if let Some(response) = shared.try_take_response().into_nyquest_result()? {
                return Ok(NSUrlSessionBlockingResponse {
                    inner: NSUrlSessionResponse {
                        response,
                        task,
                        shared,
                    },
                    max_response_buffer_size: self.inner.max_response_buffer_size,
                });
            }
            unsafe {
                task.error().into_nyquest_result()?;
            }
            std::thread::park();
        }
    }
}

impl BlockingBackend for NSUrlSessionBackend {
    type BlockingClient = NSUrlSessionBlockingClient;

    fn create_blocking_client(
        &self,
        options: ClientOptions,
    ) -> NyquestResult<Self::BlockingClient> {
        Ok(NSUrlSessionBlockingClient {
            inner: NSUrlSessionClient::create(options)?,
        })
    }
}

#[allow(irrefutable_let_patterns)]
fn coerce_waker(waker: &GenericWaker) -> &BlockingWaker {
    if let GenericWaker::Blocking(waker) = waker {
        waker
    } else {
        unreachable!("should not be called in blocking context")
    }
}
