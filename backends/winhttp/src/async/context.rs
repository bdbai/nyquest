//! Async request context and state management.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::task::Waker;

use crate::error::WinHttpError;
use crate::handle::{ConnectionHandle, RequestHandle};

/// Request states for the async state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    /// SendRequest completed, ready for writing (streaming upload)
    SendComplete = 10,
    /// WriteData completed, ready for more writing or headers
    WriteComplete = 11,
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
            10 => RequestState::SendComplete,
            11 => RequestState::WriteComplete,
            _ => RequestState::Error,
        }
    }
}

/// Inner state for a request context (protected by mutex).
struct RequestContextInner {
    /// The waker to notify when state changes
    waker: Option<Waker>,
    /// Error that occurred, if any
    error: Option<WinHttpError>,
    /// Data buffer for reads (used by async-stream feature)
    data_buffer: Vec<u8>,
    /// Active read buffer - must be kept alive until READ_COMPLETE callback fires
    /// Boxed to ensure stable address even when mutex is unlocked
    read_buffer: Option<Box<Vec<u8>>>,
    /// Request body data - must be kept alive until SENDREQUEST_COMPLETE
    request_body: Option<Vec<u8>>,
    /// Write buffer for streaming uploads - must be kept alive until WRITE_COMPLETE
    write_buffer: Option<Vec<u8>>,
    /// Whether this is a streaming upload (affects callback behavior)
    streaming_upload: bool,
    /// HTTP status code (set after headers received)
    status_code: u32,
    /// Content length (set after headers received)
    content_length: Option<u64>,
    /// Response headers (set after headers received)
    headers: Vec<(String, String)>,
    /// Connection handle (kept alive while request is active)
    connection: Option<ConnectionHandle>,
    /// Request handle
    request: Option<RequestHandle>,
}

/// Shared state for an async request.
///
/// This is the context that is passed to WinHTTP callbacks and shared between
/// the Future and the callback.
pub(crate) struct RequestContext {
    /// Current state of the request (atomic for lock-free access from callbacks)
    state: AtomicU32,
    /// Number of bytes available (atomic for lock-free access from callbacks)
    bytes_available: AtomicU32,
    /// Inner state protected by mutex
    inner: Mutex<RequestContextInner>,
}

impl RequestContext {
    /// Creates a new request context.
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self {
            state: AtomicU32::new(RequestState::Initial as u32),
            bytes_available: AtomicU32::new(0),
            inner: Mutex::new(RequestContextInner {
                waker: None,
                error: None,
                data_buffer: Vec::new(),
                read_buffer: None,
                request_body: None,
                write_buffer: None,
                streaming_upload: false,
                status_code: 0,
                content_length: None,
                headers: Vec::new(),
                connection: None,
                request: None,
            }),
        })
    }

    /// Sets whether this is a streaming upload.
    pub(crate) fn set_streaming_upload(&self, streaming: bool) {
        self.inner.lock().unwrap().streaming_upload = streaming;
    }

    /// Returns whether this is a streaming upload.
    pub(crate) fn is_streaming_upload(&self) -> bool {
        self.inner.lock().unwrap().streaming_upload
    }

    /// Returns the current state.
    pub(crate) fn state(&self) -> RequestState {
        RequestState::from(self.state.load(Ordering::Acquire))
    }

    /// Sets the state and wakes the waker if registered.
    pub(crate) fn set_state(&self, state: RequestState) {
        self.state.store(state as u32, Ordering::Release);
        let waker = self.inner.lock().unwrap().waker.take();
        if let Some(waker) = waker {
            waker.wake();
        }
    }

    /// Sets the state without waking or clearing the waker.
    /// Use this when you want to update the state but keep the waker for later notification.
    #[allow(dead_code)]
    pub(crate) fn set_state_no_wake(&self, state: RequestState) {
        self.state.store(state as u32, Ordering::Release);
    }

    /// Transitions state from expected to new state without waking.
    /// Returns true if the transition was successful.
    pub(crate) fn transition_state_no_wake(&self, from: RequestState, to: RequestState) -> bool {
        self.state
            .compare_exchange(from as u32, to as u32, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    /// Transitions state from expected to new state and wakes the waker if successful.
    /// Returns true if the transition was successful.
    pub(crate) fn transition_state(&self, from: RequestState, to: RequestState) -> bool {
        let result = self
            .state
            .compare_exchange(from as u32, to as u32, Ordering::AcqRel, Ordering::Acquire)
            .is_ok();

        if result {
            let waker = self.inner.lock().unwrap().waker.take();
            if let Some(waker) = waker {
                waker.wake();
            }
        }
        result
    }

    /// Sets the waker for notifications.
    pub(crate) fn set_waker(&self, waker: Waker) {
        self.inner.lock().unwrap().waker = Some(waker);
    }

    /// Wakes the registered waker.
    #[allow(dead_code)]
    pub(crate) fn wake(&self) {
        let waker = self.inner.lock().unwrap().waker.take();
        if let Some(waker) = waker {
            waker.wake();
        }
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

    /// Sets the request handles.
    pub(crate) fn set_handles(&self, connection: ConnectionHandle, request: RequestHandle) {
        let mut inner = self.inner.lock().unwrap();
        inner.connection = Some(connection);
        inner.request = Some(request);
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

    /// Gets a reference to the request handle.
    ///
    /// # Panics
    /// Panics if the request handle is not set.
    pub(crate) fn with_request<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&RequestHandle) -> R,
    {
        let inner = self.inner.lock().unwrap();
        f(inner.request.as_ref().expect("request handle not set"))
    }

    /// Gets the raw request handle pointer.
    ///
    /// # Panics
    /// Panics if the request handle is not set.
    pub(crate) fn get_request_raw(&self) -> *mut std::ffi::c_void {
        let inner = self.inner.lock().unwrap();
        inner
            .request
            .as_ref()
            .expect("request handle not set")
            .as_raw()
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
        inner.read_buffer = Some(Box::new(buffer));
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

    /// Gets the HTTP status code.
    pub(crate) fn status_code(&self) -> u32 {
        self.inner.lock().unwrap().status_code
    }

    /// Gets the content length.
    pub(crate) fn content_length(&self) -> Option<u64> {
        self.inner.lock().unwrap().content_length
    }

    /// Gets a copy of the response headers.
    pub(crate) fn headers(&self) -> Vec<(String, String)> {
        self.inner.lock().unwrap().headers.clone()
    }

    /// Sets all response metadata at once (status, content_length, headers).
    pub(crate) fn set_response_metadata(
        &self,
        status: u32,
        content_length: Option<u64>,
        headers: Vec<(String, String)>,
    ) {
        let mut inner = self.inner.lock().unwrap();
        inner.status_code = status;
        inner.content_length = content_length;
        inner.headers = headers;
    }
}

impl Drop for RequestContext {
    fn drop(&mut self) {
        // Clear the callback context before the handles are dropped to prevent use-after-free.
        // When WinHttpCloseHandle is called (during normal Drop), it may trigger final callbacks
        // on the Windows thread pool. By clearing the context first, those callbacks will
        // see context == 0 and return early instead of accessing freed memory.
        //
        // IMPORTANT: We must NOT hold the lock while clearing the context, because
        // if a callback is already queued and waiting for the lock, we would deadlock.
        let request_handle = {
            let inner = self.inner.lock().unwrap();
            inner.request.as_ref().map(|r| r.as_raw())
        };

        if let Some(handle) = request_handle {
            unsafe {
                // Set context to 0 to indicate the context is no longer valid.
                // This prevents callbacks that fire during or after WinHttpCloseHandle
                // from accessing the freed RequestContext.
                use windows_sys::Win32::Networking::WinHttp::WinHttpSetOption;
                use windows_sys::Win32::Networking::WinHttp::WINHTTP_OPTION_CONTEXT_VALUE;

                let zero_context: usize = 0;
                let _ = WinHttpSetOption(
                    handle,
                    WINHTTP_OPTION_CONTEXT_VALUE,
                    &zero_context as *const _ as *const _,
                    std::mem::size_of::<usize>() as u32,
                );
            }
        }

        // Now the handles will be dropped normally when RequestContextInner is dropped,
        // and any callbacks that fire will see context == 0 and return early.
    }
}
