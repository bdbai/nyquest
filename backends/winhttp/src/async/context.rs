//! Async request context and state management.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::task::Waker;

use crate::error::WinHttpError;
use crate::handle::{ConnectionHandle, RequestHandle};

/// Request states for the async state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub(crate) enum RequestState {
    /// Initial state, not yet started
    Initial = 0,
    /// Waiting for WinHttpSendRequest to complete
    Sending = 1,
    /// Waiting for WinHttpWriteData to complete (streaming upload)
    Writing = 2,
    /// Waiting for WinHttpReceiveResponse to complete
    ReceivingResponse = 3,
    /// Response headers received, ready to read data
    HeadersReceived = 4,
    /// Waiting for WinHttpQueryDataAvailable to complete
    QueryingData = 5,
    /// Data available, ready to read
    DataAvailable = 6,
    /// Waiting for WinHttpReadData to complete
    Reading = 7,
    /// Request completed successfully
    Completed = 8,
    /// Request failed with an error
    Error = 9,
}

impl From<u32> for RequestState {
    fn from(value: u32) -> Self {
        match value {
            0 => RequestState::Initial,
            1 => RequestState::Sending,
            2 => RequestState::Writing,
            3 => RequestState::ReceivingResponse,
            4 => RequestState::HeadersReceived,
            5 => RequestState::QueryingData,
            6 => RequestState::DataAvailable,
            7 => RequestState::Reading,
            8 => RequestState::Completed,
            9 => RequestState::Error,
            _ => RequestState::Error,
        }
    }
}

/// Shared state for an async request.
///
/// This is the context that is passed to WinHTTP callbacks and shared between
/// the Future and the callback.
pub(crate) struct RequestContext {
    /// Current state of the request
    state: AtomicU32,
    /// The waker to notify when state changes
    waker: Mutex<Option<Waker>>,
    /// Error that occurred, if any
    error: Mutex<Option<WinHttpError>>,
    /// Data buffer for reads (used by async-stream feature)
    #[allow(dead_code)]
    pub(crate) data_buffer: Mutex<Vec<u8>>,
    /// Request body data - must be kept alive until SENDREQUEST_COMPLETE
    request_body: Mutex<Option<Vec<u8>>>,
    /// Number of bytes available (from WinHttpQueryDataAvailable)
    pub(crate) bytes_available: AtomicU32,
    /// HTTP status code (set after headers received)
    pub(crate) status_code: AtomicU32,
    /// Content length (set after headers received)
    pub(crate) content_length: Mutex<Option<u64>>,
    /// Response headers (set after headers received)
    pub(crate) headers: Mutex<Vec<(String, String)>>,
    /// Connection handle (kept alive while request is active)
    pub(crate) connection: Mutex<Option<ConnectionHandle>>,
    /// Request handle
    pub(crate) request: Mutex<Option<RequestHandle>>,
}

impl RequestContext {
    /// Creates a new request context.
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self {
            state: AtomicU32::new(RequestState::Initial as u32),
            waker: Mutex::new(None),
            error: Mutex::new(None),
            data_buffer: Mutex::new(Vec::new()),
            request_body: Mutex::new(None),
            bytes_available: AtomicU32::new(0),
            status_code: AtomicU32::new(0),
            content_length: Mutex::new(None),
            headers: Mutex::new(Vec::new()),
            connection: Mutex::new(None),
            request: Mutex::new(None),
        })
    }

    /// Returns the current state.
    pub(crate) fn state(&self) -> RequestState {
        RequestState::from(self.state.load(Ordering::Acquire))
    }

    /// Sets the state and wakes the waker if registered.
    pub(crate) fn set_state(&self, state: RequestState) {
        self.state.store(state as u32, Ordering::Release);
        self.wake();
    }

    /// Transitions state from expected to new state.
    /// Returns true if the transition was successful.
    pub(crate) fn transition_state(&self, from: RequestState, to: RequestState) -> bool {
        self.state
            .compare_exchange(from as u32, to as u32, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    /// Sets the waker for notifications.
    pub(crate) fn set_waker(&self, waker: Waker) {
        let mut guard = self.waker.lock().unwrap();
        *guard = Some(waker);
    }

    /// Wakes the registered waker.
    pub(crate) fn wake(&self) {
        if let Some(waker) = self.waker.lock().unwrap().take() {
            waker.wake();
        }
    }

    /// Sets an error and transitions to error state.
    pub(crate) fn set_error(&self, error: WinHttpError) {
        *self.error.lock().unwrap() = Some(error);
        self.set_state(RequestState::Error);
    }

    /// Takes the error if one occurred.
    pub(crate) fn take_error(&self) -> Option<WinHttpError> {
        self.error.lock().unwrap().take()
    }

    /// Sets the request handles.
    pub(crate) fn set_handles(&self, connection: ConnectionHandle, request: RequestHandle) {
        *self.connection.lock().unwrap() = Some(connection);
        *self.request.lock().unwrap() = Some(request);
    }

    /// Sets the request body data. This must be kept alive until SENDREQUEST_COMPLETE.
    pub(crate) fn set_body(&self, body: Option<Vec<u8>>) {
        *self.request_body.lock().unwrap() = body;
    }

    /// Clears the request body data after it's no longer needed.
    pub(crate) fn clear_body(&self) {
        *self.request_body.lock().unwrap() = None;
    }

    /// Gets a pointer to the request body data.
    /// Returns (ptr, len) - ptr is null if no body.
    pub(crate) fn get_body_ptr(&self) -> (*const u8, usize) {
        let guard = self.request_body.lock().unwrap();
        match guard.as_ref() {
            Some(data) => (data.as_ptr(), data.len()),
            None => (std::ptr::null(), 0),
        }
    }

    /// Gets a reference to the request handle.
    ///
    /// # Panics
    /// Panics if the request handle is not set.
    pub(crate) fn with_request<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&RequestHandle) -> R,
    {
        let guard = self.request.lock().unwrap();
        f(guard.as_ref().expect("request handle not set"))
    }

    /// Gets the raw request handle pointer.
    ///
    /// # Panics
    /// Panics if the request handle is not set.
    pub(crate) fn get_request_raw(&self) -> *mut std::ffi::c_void {
        let guard = self.request.lock().unwrap();
        guard.as_ref().expect("request handle not set").as_raw()
    }

    /// Appends data to the buffer.
    #[cfg(feature = "async-stream")]
    pub(crate) fn append_data(&self, data: &[u8]) {
        self.data_buffer.lock().unwrap().extend_from_slice(data);
    }

    /// Consumes data from the buffer into the provided slice.
    /// Returns the number of bytes consumed.
    #[cfg(feature = "async-stream")]
    pub(crate) fn consume_data(&self, buf: &mut [u8]) -> usize {
        let mut data = self.data_buffer.lock().unwrap();
        let len = data.len().min(buf.len());
        buf[..len].copy_from_slice(&data[..len]);
        data.drain(..len);
        len
    }

    /// Returns true if there's data in the buffer.
    #[cfg(feature = "async-stream")]
    pub(crate) fn has_data(&self) -> bool {
        !self.data_buffer.lock().unwrap().is_empty()
    }
}
