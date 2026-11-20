//! Cookie types shared between the nyquest facade and backends.

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
