//! WinHTTP session management for client instances.

use std::sync::Arc;

use nyquest_interface::client::ClientOptions;

use crate::error::Result;
use crate::handle::SessionHandle;

/// Shared session state for WinHTTP clients.
pub(crate) struct WinHttpSession {
    pub(crate) session: SessionHandle,
    pub(crate) options: ClientOptions,
    pub(crate) base_cwurl: Option<Vec<u16>>,
}

impl WinHttpSession {
    /// Creates a new blocking WinHTTP session.
    pub(crate) fn new(options: ClientOptions, is_async: bool) -> Result<Arc<Self>> {
        let session = SessionHandle::new(
            options.user_agent.as_deref(),
            is_async,
            options.use_default_proxy,
        )?;
        Self::configure_session(&session, &options)?;
        let base_cwurl = options
            .base_url
            .as_deref()
            .map(|url| url.encode_utf16().chain(std::iter::once(0)).collect());
        Ok(Arc::new(Self {
            session,
            options,
            base_cwurl,
        }))
    }

    fn configure_session(session: &SessionHandle, options: &ClientOptions) -> Result<()> {
        if let Some(timeout) = options.request_timeout {
            let timeout_ms = timeout.as_millis() as i32;
            // FIXME: there is no single timeout setting for the entire request lifecycle in WinHTTP. Setting all of
            // them to the same value for simplicity, but this may not be ideal in all cases. The actual timeout could
            // be 5 * timeout_ms.
            session.set_timeouts(timeout_ms, timeout_ms, timeout_ms, timeout_ms)?;
            session.set_receive_response_timeout(timeout_ms as u32)?;
        }

        if options.follow_redirects {
            session.enable_redirects()?;
        } else {
            session.disable_redirects()?;
        }

        Ok(())
    }
}
