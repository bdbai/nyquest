//! Configuration options for HTTP clients.

use std::{borrow::Cow, time::Duration};

/// Defines how the HTTP client should handle response caching.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CachingBehavior {
    /// Caching is completely disabled.
    Disabled,
    /// Best effort caching behavior based on the backend capabilities.
    #[default]
    BestEffort,
}

/// Configuration options for proxy settings.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum ProxyOptions {
    /// Use the system's default proxy settings.
    #[default]
    Default,
    /// Disable system's default proxy settings and do not use any proxy.
    None,
    /// Use custom proxy settings.
    Custom {
        /// The proxy URL to use for HTTP requests.
        proxy_url_for_http: Cow<'static, str>,
        /// The proxy URL to use for HTTPS requests.
        proxy_url_for_https: Option<Cow<'static, str>>,
        /// Optional list of host patterns that should bypass the proxy.
        proxy_bypass: Option<Cow<'static, str>>,
    },
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
    /// Configuration options for proxy settings.
    pub proxy_options: ProxyOptions,
    /// Whether to enable cookie handling.
    pub use_cookies: bool,
    /// Whether to automatically follow redirect responses.
    pub follow_redirects: bool,
    /// Optional maximum buffer size for response bodies.
    pub max_response_buffer_size: Option<u64>,
    /// Optional timeout duration for requests.
    pub request_timeout: Option<Duration>,
    /// Whether to ignore SSL certificate errors.
    pub ignore_certificate_errors: bool,
    // TODO: auth
    // TODO: redirects
}

impl Default for ClientOptions {
    #[inline]
    fn default() -> Self {
        Self {
            base_url: None,
            user_agent: None,
            default_headers: vec![],
            caching_behavior: CachingBehavior::default(),
            proxy_options: ProxyOptions::default(),
            use_cookies: true,
            follow_redirects: true,
            max_response_buffer_size: None,
            request_timeout: None,
            ignore_certificate_errors: false,
        }
    }
}
