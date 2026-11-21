use std::sync::RwLock;
use std::time::SystemTime;

use bytes::Bytes;
use http::HeaderValue;
use nyquest_interface::{Cookie, Error as NyquestError, Result as NyquestResult};
use url::Url;

use crate::client::ReqwestClient;
use crate::cookie_util::{decode_cookie_value, encode_cookie_value};

/// A [`reqwest::cookie::CookieStore`] implementation backed directly by
/// [`cookie_store::CookieStore`].
///
/// This mirrors the behavior of [`reqwest::cookie::Jar`] (see
/// `reqwest/src/cookie.rs`) for request/response handling, while additionally
/// exposing full cookie attributes via [`cookie_store::CookieStore::matches`]
/// so `get_cookies_for` does not have to hand-parse the `Cookie` request
/// header.
#[derive(Debug, Default)]
pub(crate) struct NyquestCookieStore(RwLock<cookie_store::CookieStore>);

impl reqwest::cookie::CookieStore for NyquestCookieStore {
    fn set_cookies(&self, cookie_headers: &mut dyn Iterator<Item = &HeaderValue>, url: &Url) {
        let Ok(mut store) = self.0.write() else {
            return;
        };
        for header in cookie_headers {
            // Match `Jar`'s UTF-8 handling: ignore non-UTF-8 Set-Cookie values.
            if let Ok(header_str) = std::str::from_utf8(header.as_bytes()) {
                let _ = store.parse(header_str, url);
            }
        }
    }

    fn cookies(&self, url: &Url) -> Option<HeaderValue> {
        let store = self.0.read().ok()?;
        let s = store
            .get_request_values(url)
            .map(|(name, value)| format!("{name}={value}"))
            .collect::<Vec<_>>()
            .join("; ");
        if s.is_empty() {
            return None;
        }
        HeaderValue::from_maybe_shared(Bytes::from(s)).ok()
    }
}

impl NyquestCookieStore {
    pub(crate) fn add_cookie_str(&self, cookie: &str, url: &Url) {
        if let Ok(mut store) = self.0.write() {
            let _ = store.parse(cookie, url);
        }
    }

    fn cookies_for(&self, url: &Url) -> Vec<Cookie> {
        let Ok(store) = self.0.read() else {
            return Vec::new();
        };
        store
            .matches(url)
            .into_iter()
            .map(|c| Cookie {
                name: c.name().to_owned(),
                value: decode_cookie_value(c.value()),
                domain: c
                    .domain
                    .as_cow()
                    .map(|cow| cow.into_owned())
                    .unwrap_or_else(|| url.host_str().unwrap_or_default().to_owned())
                    .trim_start_matches('.')
                    .to_owned(),
                path: String::from(c.path.clone()),
                expires: match &c.expires {
                    cookie_store::CookieExpiration::AtUtc(dt) => Some(SystemTime::from(*dt)),
                    cookie_store::CookieExpiration::SessionEnd => None,
                },
                is_http_only: c.http_only().unwrap_or(false),
                is_secure: c.secure().unwrap_or(false),
            })
            .collect()
    }
}

impl ReqwestClient {
    pub(crate) fn add_cookie(&self, name: &str, value: &str, url: &str) -> NyquestResult<()> {
        let url = self.resolve_url(url)?;
        let value = encode_cookie_value(value);
        // Let `cookie_store` compute the default-path from `url` per RFC 6265
        // Section 5.1.4, matching the previous hand-rolled behavior.
        let cookie = format!("{name}={value}");
        self.cookie_jar.add_cookie_str(&cookie, &url);
        Ok(())
    }

    pub(crate) fn get_cookies_for(&self, url: &str) -> NyquestResult<Vec<Cookie>> {
        let url = self.resolve_url(url)?;
        Ok(self.cookie_jar.cookies_for(&url))
    }

    fn resolve_url(&self, url: &str) -> NyquestResult<Url> {
        if let Ok(url) = Url::parse(url) {
            return Ok(url);
        }
        self.base_url
            .as_ref()
            .and_then(|base_url| base_url.join(url).ok())
            .ok_or(NyquestError::InvalidUrl)
    }
}
