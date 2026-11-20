use std::time::SystemTime;

/// A single HTTP cookie.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cookie {
    /// The name of the cookie.
    pub name: String,
    /// The value of the cookie.
    pub value: String,
    /// The domain the cookie applies to.
    pub domain: String,
    /// The path the cookie applies to.
    pub path: String,
    /// The expiration time of the cookie, if it is not a session cookie.
    pub expires: Option<SystemTime>,
    /// Whether the cookie is marked as HttpOnly.
    pub is_http_only: bool,
    /// Whether the cookie is marked as Secure.
    pub is_secure: bool,
}

impl From<nyquest_interface::Cookie> for Cookie {
    fn from(cookie: nyquest_interface::Cookie) -> Self {
        Self {
            name: cookie.name,
            value: cookie.value,
            domain: cookie.domain,
            path: cookie.path,
            expires: cookie.expires,
            is_http_only: cookie.is_http_only,
            is_secure: cookie.is_secure,
        }
    }
}
