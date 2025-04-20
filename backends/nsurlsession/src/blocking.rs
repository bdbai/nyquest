use nyquest_interface::blocking::{BlockingBackend, BlockingClient, BlockingResponse, Request};
use nyquest_interface::client::{BuildClientResult, ClientOptions};
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
}

impl std::io::Read for NSUrlSessionBlockingResponse {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        todo!()
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
        todo!()
    }

    fn bytes(&mut self) -> nyquest_interface::Result<Vec<u8>> {
        let inner_waker = coerce_waker(&self.inner.shared.waker_ref());
        unsafe {
            self.inner.task.resume();
        }
        inner_waker.register_current_thread();

        while !self.inner.shared.is_completed() {
            std::thread::park();
        }
        unsafe {
            self.inner.task.error().into_nyquest_result()?;
        }
        Ok(self.inner.shared.take_response_buffer())
    }
}

impl BlockingClient for NSUrlSessionBlockingClient {
    type Response = NSUrlSessionBlockingResponse;

    fn request(&self, req: Request) -> nyquest_interface::Result<Self::Response> {
        let task = self.inner.build_data_task(req)?;
        let shared = unsafe {
            let delegate = DataTaskDelegate::new(GenericWaker::Blocking(
                BlockingWaker::new_from_current_thread(),
            ));
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
    ) -> BuildClientResult<Self::BlockingClient> {
        Ok(NSUrlSessionBlockingClient {
            inner: NSUrlSessionClient::create(options)?,
        })
    }
}

fn coerce_waker(waker: &GenericWaker) -> &BlockingWaker {
    if let GenericWaker::Blocking(waker) = waker {
        waker
    } else {
        unreachable!("should not be called in blocking context")
    }
}
