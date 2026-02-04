//! WinHTTP session management for client instances.

use std::sync::Arc;

use nyquest_interface::client::ClientOptions;

use crate::error::Result;
use crate::handle::SessionHandle;

/// Shared session state for WinHTTP clients.
pub(crate) struct WinHttpSession {
    pub(crate) session: SessionHandle,
    pub(crate) options: ClientOptions,
}

impl WinHttpSession {
    /// Creates a new blocking WinHTTP session.
    pub(crate) fn new_blocking(options: ClientOptions) -> Result<Arc<Self>> {
        let session = SessionHandle::new(options.user_agent.as_deref())?;
        Self::configure_session(&session, &options)?;
        Ok(Arc::new(Self { session, options }))
    }

    /// Creates a new async WinHTTP session.
    pub(crate) fn new_async(options: ClientOptions) -> Result<Arc<Self>> {
        let session = SessionHandle::new_async(options.user_agent.as_deref())?;
        Self::configure_session(&session, &options)?;
        Ok(Arc::new(Self { session, options }))
    }

    fn configure_session(session: &SessionHandle, options: &ClientOptions) -> Result<()> {
        // Set timeouts if specified
        if let Some(timeout) = options.request_timeout {
            let timeout_ms = timeout.as_millis() as i32;
            // Set per-phase timeouts
            session.set_timeouts(timeout_ms, timeout_ms, timeout_ms, timeout_ms)?;
            // Also set receive response timeout which controls how long to wait for the
            // server to start sending a response after the request is sent
            session.set_receive_response_timeout(timeout_ms as u32)?;
        }

        // Configure redirects
        if options.follow_redirects {
            session.enable_redirects()?;
        } else {
            session.disable_redirects()?;
        }

        Ok(())
    }

    /// Returns the maximum response buffer size, defaulting to 100MB.
    pub(crate) fn max_response_buffer_size(&self) -> u64 {
        self.options
            .max_response_buffer_size
            .unwrap_or(100 * 1024 * 1024)
    }
}
