use std::time::{Duration, SystemTime, UNIX_EPOCH};

use nyquest_interface::{Cookie, Error as NyquestError, Result as NyquestResult};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_foundation::{
    NSDictionary, NSHTTPCookie, NSHTTPCookieDomain, NSHTTPCookieName, NSHTTPCookiePath,
    NSHTTPCookieValue, NSMutableDictionary, NSString, NSURL,
};

use crate::client::NSUrlSessionClient;

impl NSUrlSessionClient {
    pub(crate) fn add_cookie_impl(&self, name: &str, value: &str, url: &str) -> NyquestResult<()> {
        let url = self.resolve_url(url)?;
        let host = url.host().ok_or(NyquestError::InvalidUrl)?.to_string();
        let path = url
            .path()
            .map(|path| default_cookie_path(&path.to_string()).to_owned())
            .unwrap_or_else(|| "/".to_owned());

        let cookie = unsafe {
            let properties: Retained<NSMutableDictionary<NSString>> = NSMutableDictionary::new();
            properties.insert(NSHTTPCookieName, &NSString::from_str(name));
            properties.insert(NSHTTPCookieDomain, &NSString::from_str(&host));
            properties.insert(NSHTTPCookiePath, &NSString::from_str(&path));
            properties.insert(NSHTTPCookieValue, &NSString::from_str(value));
            let properties: Retained<NSDictionary<NSString, AnyObject>> =
                Retained::cast_unchecked(properties.into_super());
            NSHTTPCookie::cookieWithProperties(&properties)
        }
        .ok_or_else(|| {
            NyquestError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "the cookie was rejected by the platform cookie storage",
            ))
        })?;

        let Some(storage) = &self.cookie_storage else {
            return Err(NyquestError::Io(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "the session does not have a cookie storage",
            )));
        };
        storage.setCookie(&cookie);
        Ok(())
    }

    pub(crate) fn get_cookies_for_impl(&self, url: &str) -> NyquestResult<Vec<Cookie>> {
        let url = self.resolve_url(url)?;
        let Some(storage) = &self.cookie_storage else {
            return Ok(Vec::new());
        };
        let Some(cookies) = storage.cookiesForURL(&url) else {
            return Ok(Vec::new());
        };
        Ok((0..cookies.count())
            .map(|index| cookie_to_nyquest(&cookies.objectAtIndex(index)))
            .collect())
    }

    fn resolve_url(&self, url: &str) -> NyquestResult<Retained<NSURL>> {
        NSURL::URLWithString_relativeToURL(&NSString::from_str(url), self.base_url.as_deref())
            .ok_or(NyquestError::InvalidUrl)
    }
}

fn cookie_to_nyquest(cookie: &NSHTTPCookie) -> Cookie {
    Cookie {
        name: cookie.name().to_string(),
        value: cookie.value().to_string(),
        domain: cookie
            .domain()
            .to_string()
            .trim_start_matches('.')
            .to_owned(),
        path: cookie.path().to_string(),
        expires: cookie
            .expiresDate()
            .and_then(|date| system_time_from_interval(date.timeIntervalSince1970())),
        is_http_only: cookie.isHTTPOnly(),
        is_secure: cookie.isSecure(),
    }
}

fn system_time_from_interval(interval: f64) -> Option<SystemTime> {
    if !interval.is_finite() || interval < 0.0 {
        return None;
    }
    Some(UNIX_EPOCH + Duration::from_secs_f64(interval))
}

fn default_cookie_path(path: &str) -> &str {
    match path.rfind('/') {
        Some(0) | None => "/",
        Some(index) => &path[..index],
    }
}
