use std::borrow::Cow;

/// Custom proxy configuration for HTTP clients.
#[derive(Debug, Clone)]
pub struct CustomProxy {
    pub(super) proxy_url_for_http: Cow<'static, str>,
    pub(super) proxy_url_for_https: Option<Cow<'static, str>>,
    pub(super) proxy_bypass: Option<Cow<'static, str>>,
}

impl CustomProxy {
    /// Creates a new [`CustomProxy`] with the specified proxy URL for HTTP requests.
    pub fn new(proxy_url_for_http: impl Into<Cow<'static, str>>) -> Self {
        Self {
            proxy_url_for_http: proxy_url_for_http.into(),
            proxy_url_for_https: None,
            proxy_bypass: None,
        }
    }

    /// Sets the proxy URL for HTTPS requests.
    ///
    /// # Note
    ///
    /// Some backends like libcurl do not support separate proxy URLs for HTTP and HTTPS. In such
    /// cases, the provided HTTPS proxy URL may be ignored, and the HTTP proxy URL will be used for
    /// both HTTP and HTTPS requests.
    pub fn with_https_proxy(mut self, proxy_url_for_https: impl Into<Cow<'static, str>>) -> Self {
        self.proxy_url_for_https = Some(proxy_url_for_https.into());
        self
    }

    /// Sets the list of host patterns that should bypass the proxy.
    pub fn with_bypass(mut self, proxy_bypass: impl Into<Cow<'static, str>>) -> Self {
        self.proxy_bypass = Some(proxy_bypass.into());
        self
    }
}
