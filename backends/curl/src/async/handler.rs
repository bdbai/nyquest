use std::sync::Arc;

use curl::easy::WriteError;

use super::pause::EasyPause;
use super::shared::SharedRequestStates;
use crate::curl_ng::easy::EasyCallback;

#[derive(Default)]
pub(crate) struct AsyncHandler {
    pub(super) ctx: Arc<SharedRequestStates>,
    // To be filled after Easy2 is constructed
    pub(crate) pause: Option<EasyPause>,
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
        let mut state = inner.ctx.state.lock().unwrap();
        inner.ctx.waker.wake();
        #[cfg(feature = "ws")]
        if let Some(ws) = &mut state.ws {
            return ws.on_write(data, inner.pause.raw());
        }
        let state = &mut state.state;
        if state.data_available() {
            Err(WriteError::Pause)
        } else {
            state.write_data(data);
            Ok(data.len())
        }
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
                    if inner.pause.pause_recv().is_err() {
                        return false;
                    }
                }
            }
        }
        inner.ctx.waker.wake();
        true
    }

    #[cfg(any(feature = "async-stream", feature = "ws"))]
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, curl::easy::ReadError> {
        let Some(inner) = self.get_ref() else {
            return Err(curl::easy::ReadError::Abort);
        };
        let mut state = inner.ctx.state.lock().unwrap();
        #[cfg(feature = "ws")]
        if let Some(ws) = &mut state.ws {
            return ws.on_read(buf, inner.pause.raw());
        }
        #[cfg(feature = "async-stream")]
        {
            let stream = state
                .req_streams
                .get_mut(0)
                .ok_or(curl::easy::ReadError::Abort)?;
            stream.read(buf, inner.ctx)
        }
        #[cfg(not(feature = "async-stream"))]
        Err(curl::easy::ReadError::Abort)
    }

    #[cfg(not(any(feature = "async-stream", feature = "ws")))]
    fn read(&mut self, _buf: &mut [u8]) -> Result<usize, curl::easy::ReadError> {
        Err(curl::easy::ReadError::Abort)
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
    fn seek(&mut self, _whence: std::io::SeekFrom) -> curl::easy::SeekResult {
        curl::easy::SeekResult::Fail
    }
}
