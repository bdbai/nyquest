//! Async request context and state management.

use std::ops::Range;
use std::sync::{Arc, Mutex};
use std::task::Waker;

use crate::error::WinHttpError;

bitflags::bitflags! {
    /// Request states for the async state machine.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) struct RequestState: u16 {
        /// Initial state, not yet started
        const Initial = 0b0000_0000_0000_0001;
        /// Sent done, ready for writing body or receiving response
        const HeadersSent = 0b0000_0000_0000_0010;
        /// WriteData completed, ready for more writing or receiving response
        const WriteComplete = 0b0000_0000_0000_0100;
        /// Response headers received, ready to read data
        const HeadersReceived = 0b0000_0000_0000_1000;
        /// Waiting for WinHttpQueryDataAvailable
        const QueryingData = 0b0000_0000_0001_0000;
        /// Data available, ready to read
        const DataAvailable = 0b0000_0000_0010_0000;
        /// Waiting for WinHttpReadData to complete
        const Reading = 0b0000_0000_0100_0000;
        /// Request completed successfully
        const Completed = 0b0000_0000_1000_0000;
    }
}

/// Inner state for a request context (protected by mutex).
pub(crate) struct RequestContextInner {
    pub(crate) state: RequestState,
    /// The waker to notify when state changes
    pub(crate) waker: Waker,
    /// Error that occurred, if any
    pub(crate) error: Option<WinHttpError>,
    /// Buffer for transferring data to/from WinHTTP.
    pub(crate) buffer: Vec<u8>,
    /// For writing, it is 0..bytes_written.
    /// For query data available, it is 0..available_bytes.
    /// For reading, it is the range of valid read data in the buffer.
    pub(crate) buffer_range: Range<usize>,
}

/// Shared state for an async request.
///
/// This is the context that is passed to WinHTTP callbacks and shared between
/// the Future and the callback.
pub(crate) struct RequestContext {
    /// Inner state protected by mutex
    pub(crate) inner: Mutex<RequestContextInner>,
}

impl RequestContext {
    /// Creates a new request context.
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self {
            inner: Mutex::new(RequestContextInner {
                state: RequestState::Initial,
                waker: futures_task::noop_waker(), // FIXME: use std::task::Waker::noop() when MSRV >= 1.85
                error: None,
                buffer: Default::default(),
                buffer_range: 0..0,
            }),
        })
    }

    /// Sets the state without waking or clearing the waker.
    /// Use this when you want to update the state but keep the waker for later notification.
    #[allow(dead_code)]
    pub(crate) fn set_state_no_wake(&self, state: RequestState) {
        self.inner.lock().unwrap().state = state;
    }

    /// Transitions state from expected to new state and wakes the waker if successful.
    pub(crate) fn transition_state(&self, to: RequestState) {
        let mut inner = self.inner.lock().unwrap();
        inner.state = to;
        inner.waker.wake_by_ref();
    }

    pub(crate) fn set_send_complete(&self) {
        let mut inner = self.inner.lock().unwrap();
        inner.buffer = vec![];
        inner.state = RequestState::HeadersSent;
        inner.waker.wake_by_ref();
    }

    pub(crate) fn set_write_complete(&self, bytes_written: usize) {
        let mut inner = self.inner.lock().unwrap();
        inner.buffer_range.end += bytes_written;
        inner.state = RequestState::WriteComplete;
        inner.waker.wake_by_ref();
    }

    /// # Safety
    /// ptr must be a valid pointer to the buffer.
    /// bytes_read must be the valid number of bytes read into the buffer.
    pub(crate) unsafe fn set_read_complete(&self, ptr: *const u8, bytes_read: usize) {
        let mut inner = self.inner.lock().unwrap();
        if bytes_read == 0 {
            inner.state = RequestState::Completed
        } else {
            let new_start = ptr as usize - inner.buffer.as_ptr() as usize;
            inner.buffer_range = new_start..(new_start + bytes_read);
            inner.state = RequestState::HeadersReceived
        }
        inner.waker.wake_by_ref();
    }

    /// Sets an error and transitions to error state.
    pub(crate) fn set_error(&self, error: WinHttpError) {
        let mut inner = self.inner.lock().unwrap();
        inner.error = Some(error);
        inner.waker.wake_by_ref();
    }

    /// Takes the error if one occurred.
    pub(crate) fn take_error(&self) -> Option<WinHttpError> {
        self.inner.lock().unwrap().error.take()
    }

    /// Sets the request body data. This must be kept alive until SENDREQUEST_COMPLETE.
    pub(crate) fn set_body(&self, body: Vec<u8>) {
        self.inner.lock().unwrap().buffer = body;
    }

    /// Sets the write buffer for streaming uploads. This must be kept alive until WRITE_COMPLETE.
    pub(crate) fn set_write_buffer(&self, buffer: Vec<u8>) {
        let mut inner = self.inner.lock().unwrap();
        inner.buffer = buffer;
        inner.buffer_range = 0..0;
    }

    /// Gets a pointer to the write buffer.
    /// Returns (ptr, len) - ptr is null if no buffer.
    pub(crate) fn prepare_for_writing(&self) -> *const u8 {
        let mut inner = self.inner.lock().unwrap();
        inner.state = RequestState::HeadersSent;
        inner.buffer_range = 0..0;
        inner.buffer.as_ptr()
    }

    /// Clears the write buffer after WRITE_COMPLETE.
    pub(crate) fn take_write_buffer(&self) -> Vec<u8> {
        let mut inner = self.inner.lock().unwrap();
        inner.buffer_range = 0..0;
        std::mem::take(&mut inner.buffer)
    }

    /// Gets a pointer to the request body data.
    /// Returns (ptr, len) - ptr is null if no body.
    pub(crate) fn get_body_ptr(&self) -> (*const u8, usize) {
        let inner = self.inner.lock().unwrap();
        (inner.buffer.as_ptr(), inner.buffer.len())
    }

    /// Sets the number of bytes available.
    pub(crate) fn set_bytes_available(&self, bytes: u32) {
        self.inner.lock().unwrap().buffer_range = 0..bytes as usize;
    }
}
