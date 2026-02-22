//! Async request context and state management.

use std::sync::atomic::{AtomicU32, Ordering};
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
        const HeadersReceived = 0b0000_0000_0001_0000;
        /// Waiting for WinHttpQueryDataAvailable
        const QueryingData = 0b0000_0000_0010_0000;
        /// Data available, ready to read
        const DataAvailable = 0b0000_0000_0100_0000;
        /// Waiting for WinHttpReadData to complete
        const Reading = 0b0000_0000_1000_0000;
        /// Request completed successfully
        const Completed = 0b0000_0001_0000_0000;
        /// Request failed with an error
        const Error = 0b0000_0010_0000_0000;
    }
}

/// Inner state for a request context (protected by mutex).
struct RequestContextInner {
    state: RequestState,
    /// The waker to notify when state changes
    waker: Waker,
    /// Error that occurred, if any
    error: Option<WinHttpError>,
    /// Data buffer for reads (used by async-stream feature)
    data_buffer: Vec<u8>,
    /// Active read buffer - must be kept alive until READ_COMPLETE callback fires
    /// Boxed to ensure stable address even when mutex is unlocked
    read_buffer: Option<Vec<u8>>,
    /// Request body data - must be kept alive until SENDREQUEST_COMPLETE
    request_body: Option<Vec<u8>>,
    /// Write buffer for streaming uploads - must be kept alive until WRITE_COMPLETE
    write_buffer: Option<Vec<u8>>,
}

/// Shared state for an async request.
///
/// This is the context that is passed to WinHTTP callbacks and shared between
/// the Future and the callback.
pub(crate) struct RequestContext {
    /// Number of bytes available (atomic for lock-free access from callbacks)
    bytes_available: AtomicU32,
    /// Inner state protected by mutex
    inner: Mutex<RequestContextInner>,
}

impl RequestContext {
    /// Creates a new request context.
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self {
            bytes_available: AtomicU32::new(0),
            inner: Mutex::new(RequestContextInner {
                state: RequestState::Initial,
                waker: futures_task::noop_waker(), // FIXME: use std::task::Waker::noop() when MSRV >= 1.85
                error: None,
                data_buffer: Vec::new(),
                read_buffer: None,
                request_body: None,
                write_buffer: None,
            }),
        })
    }

    /// Returns the current state.
    pub(crate) fn state(&self) -> RequestState {
        self.inner.lock().unwrap().state
    }

    /// Sets the state and wakes the waker if registered.
    pub(crate) fn set_state(&self, state: RequestState) {
        let mut inner = self.inner.lock().unwrap();
        inner.state = state;
        inner.waker.wake_by_ref();
    }

    /// Sets the state without waking or clearing the waker.
    /// Use this when you want to update the state but keep the waker for later notification.
    #[allow(dead_code)]
    pub(crate) fn set_state_no_wake(&self, state: RequestState) {
        self.inner.lock().unwrap().state = state;
    }

    /// Transitions state from expected to new state without waking.
    pub(crate) fn transition_state_no_wake(&self, to: RequestState) {
        let mut inner = self.inner.lock().unwrap();
        inner.state = to;
    }

    /// Transitions state from expected to new state and wakes the waker if successful.
    pub(crate) fn transition_state(&self, to: RequestState) {
        let mut inner = self.inner.lock().unwrap();
        inner.state = to;
        inner.waker.wake_by_ref();
    }

    /// Sets the waker for notifications.
    pub(crate) fn set_waker(&self, waker: &Waker) {
        let mut inner = self.inner.lock().unwrap();
        inner.waker.clone_from(waker);
    }

    /// Sets an error and transitions to error state.
    pub(crate) fn set_error(&self, error: WinHttpError) {
        let mut inner = self.inner.lock().unwrap();
        inner.error = Some(error);
        drop(inner); // Release lock
        self.set_state(RequestState::Error);
    }

    /// Takes the error if one occurred.
    pub(crate) fn take_error(&self) -> Option<WinHttpError> {
        self.inner.lock().unwrap().error.take()
    }

    /// Sets the request body data. This must be kept alive until SENDREQUEST_COMPLETE.
    pub(crate) fn set_body(&self, body: Option<Vec<u8>>) {
        self.inner.lock().unwrap().request_body = body;
    }

    /// Clears the request body data after it's no longer needed.
    pub(crate) fn clear_body(&self) {
        self.inner.lock().unwrap().request_body = None;
    }

    /// Sets the write buffer for streaming uploads. This must be kept alive until WRITE_COMPLETE.
    pub(crate) fn set_write_buffer(&self, buffer: Vec<u8>) {
        self.inner.lock().unwrap().write_buffer = Some(buffer);
    }

    /// Gets a pointer to the write buffer.
    /// Returns (ptr, len) - ptr is null if no buffer.
    pub(crate) fn get_write_buffer_ptr(&self) -> (*const u8, usize) {
        let inner = self.inner.lock().unwrap();
        match inner.write_buffer.as_ref() {
            Some(data) => (data.as_ptr(), data.len()),
            None => (std::ptr::null(), 0),
        }
    }

    /// Clears the write buffer after WRITE_COMPLETE.
    pub(crate) fn clear_write_buffer(&self) {
        self.inner.lock().unwrap().write_buffer = None;
    }

    /// Gets a pointer to the request body data.
    /// Returns (ptr, len) - ptr is null if no body.
    pub(crate) fn get_body_ptr(&self) -> (*const u8, usize) {
        let inner = self.inner.lock().unwrap();
        match inner.request_body.as_ref() {
            Some(data) => (data.as_ptr(), data.len()),
            None => (std::ptr::null(), 0),
        }
    }

    /// Returns the number of bytes consumed.
    #[cfg(feature = "async-stream")]
    pub(crate) fn consume_data(&self, buf: &mut [u8]) -> usize {
        let mut inner = self.inner.lock().unwrap();
        let len = inner.data_buffer.len().min(buf.len());
        buf[..len].copy_from_slice(&inner.data_buffer[..len]);
        inner.data_buffer.drain(..len);
        len
    }

    /// Returns true if there's data in the buffer.
    #[cfg(feature = "async-stream")]
    pub(crate) fn has_data(&self) -> bool {
        !self.inner.lock().unwrap().data_buffer.is_empty()
    }

    /// Sets the active read buffer for async reads and returns a pointer to it.
    /// This buffer must be kept alive until the READ_COMPLETE callback fires.
    /// Returns the buffer pointer that should be passed to WinHttpReadData.
    /// The buffer is boxed to ensure a stable address.
    #[cfg(feature = "async-stream")]
    pub(crate) fn set_read_buffer(&self, mut buffer: Vec<u8>) -> *mut u8 {
        let mut inner = self.inner.lock().unwrap();
        // Get the pointer to the Vec's data before boxing
        let ptr = buffer.as_mut_ptr();
        inner.read_buffer = Some(buffer);
        ptr
    }

    /// Takes ownership of the read buffer and moves its data to the data buffer.
    /// Called from the READ_COMPLETE callback.
    #[cfg(feature = "async-stream")]
    pub(crate) fn complete_read(&self, bytes_read: usize) {
        let mut inner = self.inner.lock().unwrap();
        // Take the buffer and copy the data
        if let Some(buffer) = inner.read_buffer.take() {
            if bytes_read <= buffer.len() {
                inner.data_buffer.extend_from_slice(&buffer[..bytes_read]);
            }
        }
    }

    /// Gets the number of bytes available.
    pub(crate) fn bytes_available(&self) -> u32 {
        self.bytes_available.load(Ordering::Acquire)
    }

    /// Sets the number of bytes available.
    pub(crate) fn set_bytes_available(&self, bytes: u32) {
        self.bytes_available.store(bytes, Ordering::Release);
    }
}
