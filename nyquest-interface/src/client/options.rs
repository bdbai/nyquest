//! Configuration options for HTTP clients.

use std::time::Duration;

/// Defines how the HTTP client should handle response caching.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CachingBehavior {
    /// Caching is completely disabled.
    Disabled,
    /// Best effort caching behavior based on the backend capabilities.
    #[default]
    BestEffort,
}

/// Configuration options for creating a nyquest HTTP client.
#[derive(Debug, Clone)]
pub struct ClientOptions {
    /// Optional base URL prepended to all request URLs.
    pub base_url: Option<String>,
    /// Optional User-Agent header value to use for all requests.
    pub user_agent: Option<String>,
    /// Headers to include in all requests by default.
    pub default_headers: Vec<(String, String)>,
    /// Controls the caching behavior for HTTP responses.
    pub caching_behavior: CachingBehavior,
    /// Whether to use the system's default proxy settings.
    pub use_default_proxy: bool,
    /// Whether to enable cookie handling.
    pub use_cookies: bool,
    /// Whether to automatically follow redirect responses.
    pub follow_redirects: bool,
    /// Optional maximum buffer size for response bodies.
    pub max_response_buffer_size: Option<u64>,
    /// Optional timeout duration for requests.
    pub request_timeout: Option<Duration>,
    // TODO: ignore TLS validation
    // TODO: auth
    // TODO: redirects
}

impl Default for ClientOptions {
    fn default() -> Self {
        Self {
            base_url: None,
            user_agent: None,
            default_headers: vec![],
            caching_behavior: CachingBehavior::default(),
            use_default_proxy: true,
            use_cookies: true,
            follow_redirects: true,
            max_response_buffer_size: None,
            request_timeout: None,
        }
    }
}
