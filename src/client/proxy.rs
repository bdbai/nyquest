use std::borrow::Cow;

use nyquest_interface::client::ProxyOptions;

/// Custom proxy configuration for HTTP clients.
///
/// # Examples
///
/// To set a custom proxy for HTTP requests only:
/// ```rust
/// # use nyquest::client::CustomProxy;
/// let proxy = CustomProxy::http("http://my-http-proxy:8080");
/// ```
///
/// To set a custom proxy for HTTPS requests only:
/// ```rust
/// # use nyquest::client::CustomProxy;
/// let proxy = CustomProxy::https("http://my-https-proxy:8080");
/// ```
///
/// To set custom proxies for both HTTP and HTTPS requests with a bypass list:
/// ```rust
/// # use nyquest::client::CustomProxy;
/// let proxy = CustomProxy::http("http://my-http-proxy:8080")
///     .with_https("http://my-https-proxy:8080")
///     .with_bypass("excluded-domain.com");
/// ```
#[derive(Debug, Clone)]
pub struct CustomProxy<HTTP, HTTPS> {
    pub(super) http: HTTP,
    pub(super) https: HTTPS,
    pub(super) proxy_bypass: Option<Cow<'static, str>>,
}

impl CustomProxy<(), ()> {
    /// Creates a new [`CustomProxy`] with the specified proxy URL for HTTP requests.
    ///
    /// The HTTPS proxy URL can be set later using the [`Self::with_https`] method.
    pub fn http(
        proxy_url_for_http: impl Into<Cow<'static, str>>,
    ) -> CustomProxy<Cow<'static, str>, ()> {
        CustomProxy {
            http: proxy_url_for_http.into(),
            https: (),
            proxy_bypass: None,
        }
    }

    /// Creates a new [`CustomProxy`] with the specified proxy URL for HTTPS requests only.
    pub fn https(
        proxy_url_for_https: impl Into<Cow<'static, str>>,
    ) -> CustomProxy<(), Cow<'static, str>> {
        CustomProxy {
            http: (),
            https: proxy_url_for_https.into(),
            proxy_bypass: None,
        }
    }
}

impl<HTTPS> CustomProxy<Cow<'static, str>, HTTPS> {
    /// Sets the proxy URL for HTTPS requests in addition to the existing HTTP proxy URL.
    pub fn with_https(
        self,
        proxy_url_for_https: impl Into<Cow<'static, str>>,
    ) -> CustomProxy<Cow<'static, str>, Cow<'static, str>> {
        CustomProxy {
            http: self.http,
            https: proxy_url_for_https.into(),
            proxy_bypass: self.proxy_bypass,
        }
    }
}

impl<HTTP, HTTPS> CustomProxy<HTTP, HTTPS> {
    /// Sets the list of host patterns that should bypass the proxy.
    pub fn with_bypass(mut self, proxy_bypass: impl Into<Cow<'static, str>>) -> Self {
        self.proxy_bypass = Some(proxy_bypass.into());
        self
    }
}

pub trait IntoProxyOptions {
    fn into_proxy_options(self) -> ProxyOptions;
}

impl IntoProxyOptions for CustomProxy<(), Cow<'static, str>> {
    fn into_proxy_options(self) -> ProxyOptions {
        ProxyOptions::Custom {
            http: None,
            https: Some(self.https),
            proxy_bypass: self.proxy_bypass,
        }
    }
}

impl IntoProxyOptions for CustomProxy<Cow<'static, str>, ()> {
    fn into_proxy_options(self) -> ProxyOptions {
        ProxyOptions::Custom {
            http: Some(self.http),
            https: None,
            proxy_bypass: self.proxy_bypass,
        }
    }
}

impl IntoProxyOptions for CustomProxy<Cow<'static, str>, Cow<'static, str>> {
    fn into_proxy_options(self) -> ProxyOptions {
        ProxyOptions::Custom {
            http: Some(self.http),
            https: Some(self.https),
            proxy_bypass: self.proxy_bypass,
        }
    }
}
