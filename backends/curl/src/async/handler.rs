use std::sync::Arc;

use curl::easy::WriteError;

use super::pause::EasyPause;
use super::shared::SharedRequestStates;
use crate::curl_ng::easy::EasyCallback;

#[derive(Default)]
pub(super) struct AsyncHandler {
    pub(super) ctx: Arc<SharedRequestStates>,
    // To be filled after Easy2 is constructed
    pub(super) pause: Option<EasyPause>,
}

struct AsyncHandlerRef<'a> {
    ctx: &'a SharedRequestStates,
    pause: &'a mut EasyPause,
}

impl AsyncHandler {
    fn get_ref(&mut self) -> Option<AsyncHandlerRef<'_>> {
        let ctx = self.ctx.as_ref();
        let pause = self.pause.as_mut()?;
        Some(AsyncHandlerRef { ctx, pause })
    }
}

impl EasyCallback for AsyncHandler {
    fn write(&mut self, data: &[u8]) -> Result<usize, WriteError> {
        let Some(inner) = self.get_ref() else {
            // ... signals an error condition to the library and returns CURLE_WRITE_ERROR.
            return Ok(0);
        };
        {
            let mut state = inner.ctx.state.lock().unwrap();
            let state = &mut state.state;
            state.write_data(data);
        }
        unsafe {
            inner.pause.pause_recv();
        }
        inner.ctx.waker.wake();
        Ok(data.len())
    }

    fn header(&mut self, data: &[u8]) -> bool {
        let Some(inner) = self.get_ref() else {
            // ... signals an error condition to the library and returns CURLE_WRITE_ERROR.
            return false;
        };
        {
            let mut state = inner.ctx.state.lock().unwrap();
            let state = &mut state.state;
            if state.push_header_data(data) {
                unsafe {
                    inner.pause.pause_recv();
                }
            }
        }
        inner.ctx.waker.wake();
        true
    }

    #[cfg(feature = "async-stream")]
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, curl::easy::ReadError> {
        let mut state = self.ctx.state.lock().unwrap();
        let stream = state
            .req_streams
            .get_mut(0)
            .ok_or(curl::easy::ReadError::Abort)?;
        stream.read(buf, &self.ctx)
    }

    #[cfg(feature = "async-stream")]
    fn seek(&mut self, whence: std::io::SeekFrom) -> curl::easy::SeekResult {
        let mut state = self.ctx.state.lock().unwrap();
        let Some(stream) = state.req_streams.get_mut(0) else {
            return curl::easy::SeekResult::Fail;
        };
        stream.seek(whence, &self.ctx)
    }

    #[cfg(not(feature = "async-stream"))]
    fn read(&mut self, _buf: &mut [u8]) -> Result<usize, curl::easy::ReadError> {
        Err(curl::easy::ReadError::Abort)
    }

    #[cfg(not(feature = "async-stream"))]
    fn seek(&mut self, _whence: std::io::SeekFrom) -> curl::easy::SeekResult {
        curl::easy::SeekResult::Fail
    }
}
