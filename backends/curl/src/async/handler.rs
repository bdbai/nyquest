use std::sync::Arc;

use curl::easy::{Handler, WriteError};

use super::pause::EasyPause;
use super::r#loop::SharedRequestContext;

#[derive(Default)]
pub(super) struct AsyncHandler {
    // To be filled in the loop
    pub(super) ctx: Option<Arc<SharedRequestContext>>,
    // To be filled after Easy2 is constructed
    pub(super) pause: Option<EasyPause>,
}

struct AsyncHandlerRef<'a> {
    ctx: &'a SharedRequestContext,
    pause: &'a mut EasyPause,
}

impl AsyncHandler {
    fn get_ref(&mut self) -> Option<AsyncHandlerRef<'_>> {
        let ctx = self.ctx.as_ref()?;
        let pause = self.pause.as_mut()?;
        Some(AsyncHandlerRef { ctx, pause })
    }
}

impl Handler for AsyncHandler {
    fn write(&mut self, data: &[u8]) -> Result<usize, WriteError> {
        let Some(inner) = self.get_ref() else {
            // ... signals an error condition to the library and returns CURLE_WRITE_ERROR.
            return Ok(0);
        };
        {
            let mut state = inner.ctx.state.lock().unwrap();
            let state = &mut state.0;
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
            let state = &mut state.0;
            if state.push_header_data(data) {
                unsafe {
                    inner.pause.pause_recv();
                }
            }
        }
        inner.ctx.waker.wake();
        true
    }
}
