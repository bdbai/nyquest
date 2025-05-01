use std::time::Duration;

use nyquest_interface::client::{CachingBehavior, ClientOptions};

/// A builder for creating an async or blocking client with custom options.
///
/// Use [`ClientBuilder::default()`] to create a new builder instance.
#[derive(Debug, Clone, Default)]
pub struct ClientBuilder {
    pub(crate) options: ClientOptions,
}

impl ClientBuilder {
    /// Sets the base URL for the client.
    pub fn base_url(mut self, base_url: impl Into<String>) -> Self {
        self.options.base_url = Some(base_url.into());
        self
    }

    /// Sets the `user-agent` header for the client.
    pub fn user_agent(mut self, user_agent: impl Into<String>) -> Self {
        self.options.user_agent = Some(user_agent.into());
        self
    }

    /// Adds a request header to all requests made with this client.
    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.options
            .default_headers
            .push((name.into(), value.into()));
        self
    }

    /// Instructs the backend to bypass cache.
    #[inline]
    pub fn no_caching(mut self) -> Self {
        self.options.caching_behavior = CachingBehavior::Disabled;
        self
    }

    /// Instructs the backend to bypass preset proxies.
    #[inline]
    pub fn no_proxy(mut self) -> Self {
        self.options.use_default_proxy = false;
        self
    }

    /// Instructs the backend to not keep cookies between requests.
    #[inline]
    pub fn no_cookies(mut self) -> Self {
        self.options.use_cookies = false;
        self
    }

    /// Sets the maximum number of bytes to buffer for a response.
    ///
    /// # Note
    ///
    /// The limit only applies to `response.bytes()` and `response.text()`.
    /// Streaming is not affected.
    #[inline]
    pub fn max_response_buffer_size(mut self, size: u64) -> Self {
        self.options.max_response_buffer_size = Some(size);
        self
    }

    /// Sets the timeout for a whole request to complete.
    ///
    /// # Note
    ///
    /// The precision of the timeout is implementation defined.
    #[inline]
    pub fn request_timeout(mut self, timeout: Duration) -> Self {
        self.options.request_timeout = Some(timeout);
        self
    }
}
