use std::ffi::CString;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use nyquest_interface::{Cookie, Error as NyquestError, Result as NyquestResult};

use crate::curl_ng::cookie::with_share_easy;
use crate::curl_ng::easy::Share;
use crate::curl_ng::url::CurlUrl;

pub(crate) fn add_cookie(share: &Share, name: &str, value: &str, url: &str) -> NyquestResult<()> {
    let url_cstr = CString::new(url).map_err(|_| NyquestError::InvalidUrl)?;
    let url = CurlUrl::parse(&url_cstr).map_err(|_| NyquestError::InvalidUrl)?;
    let host = url.host().map_err(|_| NyquestError::InvalidUrl)?;
    let url_path = url.path().map_err(|_| NyquestError::InvalidUrl)?;
    let path = default_cookie_path(&url_path);
    // Use the Netscape format with `FALSE` for host-only cookies so that
    // single-label hosts like `localhost.` are accepted. A `Set-Cookie:` line
    // with an explicit `Domain` attribute is rejected by libcurl for such
    // hosts, while server `Set-Cookie` responses (host-only) work fine.
    let cookie = format!("{host}\tFALSE\t{path}\tFALSE\t0\t{name}\t{value}");
    let cookie = CString::new(cookie).map_err(|_| NyquestError::InvalidUrl)?;

    with_share_easy(share, |easy| easy.set_cookielist(&cookie))?;
    Ok(())
}

pub(crate) fn get_cookies_for(share: &Share, url: &str) -> NyquestResult<Vec<Cookie>> {
    let url_cstr = CString::new(url).map_err(|_| NyquestError::InvalidUrl)?;
    let url = CurlUrl::parse(&url_cstr).map_err(|_| NyquestError::InvalidUrl)?;
    let request_host = url.host().map_err(|_| NyquestError::InvalidUrl)?;
    let request_path = url.path().map_err(|_| NyquestError::InvalidUrl)?;
    let request_is_secure = url
        .scheme()
        .is_ok_and(|scheme| scheme.eq_ignore_ascii_case("https"));
    let now = SystemTime::now();

    with_share_easy(share, |easy| {
        let cookie_list = easy.get_cookielist()?;
        let mut cookies = Vec::new();
        for line in &cookie_list {
            let line = String::from_utf8_lossy(line);
            if let Some(cookie) =
                parse_cookie_line(&line, &request_host, &request_path, request_is_secure, now)
            {
                cookies.push(cookie);
            }
        }
        Ok(cookies)
    })
    .map_err(NyquestError::from)
}

fn parse_cookie_line(
    line: &str,
    request_host: &str,
    request_path: &str,
    request_is_secure: bool,
    now: SystemTime,
) -> Option<Cookie> {
    let mut fields = line.splitn(7, '\t');
    let raw_domain = fields.next()?;
    let include_subdomains = fields.next()?;
    let path = fields.next()?;
    let secure = fields.next()?;
    let expires = fields.next()?;
    let name = fields.next()?;
    let value = fields.next().unwrap_or_default();

    // libcurl encodes the HttpOnly attribute as a `#HttpOnly_` prefix on the
    // domain field of CURLINFO_COOKIELIST, rather than as a separate field.
    let (domain, is_http_only) = match raw_domain.strip_prefix("#HttpOnly_") {
        Some(rest) => (rest, true),
        None => (raw_domain, false),
    };

    let normalized_domain = normalize_cookie_domain(domain);
    if !domain_matches(&normalized_domain, include_subdomains, request_host)
        || !path_matches(path, request_path)
    {
        return None;
    }

    let is_secure = secure.eq_ignore_ascii_case("TRUE");
    if is_secure && !request_is_secure {
        return None;
    }

    let expires = expires
        .parse::<i64>()
        .ok()
        .filter(|timestamp| *timestamp > 0)
        .and_then(|timestamp| {
            let timestamp = u64::try_from(timestamp).ok()?;
            Some(UNIX_EPOCH + Duration::from_secs(timestamp))
        });
    if expires.is_some_and(|expires| expires <= now) {
        return None;
    }

    Some(Cookie {
        name: name.to_owned(),
        value: value.to_owned(),
        domain: normalized_domain.trim_start_matches('.').to_owned(),
        path: path.to_owned(),
        expires,
        is_http_only,
        is_secure,
    })
}

/// Normalizes a cookie domain from libcurl's COOKIELIST via `CURLU` so that
/// IDNA domains compare consistently with the normalized request host.
///
/// [`CurlUrl::host`] already requests `CURLU_PUNYCODE`. The domain stored by
/// libcurl may be in Unicode form when it originates from a `Domain=`
/// attribute or from `add_cookie`, so it is re-parsed through `CURLU` to
/// obtain the same punycode form. Falls back to the raw domain when
/// normalization is not possible (e.g. a libcurl build without IDN support —
/// in that case libcurl itself cannot handle the IDNA host either, so plain
/// string comparison is sufficient).
fn normalize_cookie_domain(domain: &str) -> String {
    let trimmed = domain.trim_start_matches('.');
    if trimmed.is_empty() {
        return trimmed.to_owned();
    }
    // Fast path: pure ASCII domains are already in normalized form.
    if trimmed.is_ascii() {
        return trimmed.to_owned();
    }
    let url_str = format!("http://{trimmed}/");
    let Ok(url_cstr) = CString::new(url_str) else {
        return trimmed.to_owned();
    };
    let Ok(url) = CurlUrl::parse(&url_cstr) else {
        return trimmed.to_owned();
    };
    url.host().unwrap_or_else(|_| trimmed.to_owned())
}

fn domain_matches(domain: &str, include_subdomains: &str, request_host: &str) -> bool {
    let domain = domain.trim_start_matches('.');
    if request_host.eq_ignore_ascii_case(domain) {
        return true;
    }
    if include_subdomains.eq_ignore_ascii_case("TRUE") {
        return request_host.len() > domain.len()
            && request_host
                .get(request_host.len() - domain.len()..)
                .is_some_and(|suffix| suffix.eq_ignore_ascii_case(domain))
            && request_host.as_bytes()[request_host.len() - domain.len() - 1] == b'.';
    }
    false
}

fn path_matches(cookie_path: &str, request_path: &str) -> bool {
    if cookie_path == "/" {
        return true;
    }
    if request_path == cookie_path {
        return true;
    }
    request_path
        .strip_prefix(cookie_path)
        .is_some_and(|rest| rest.starts_with('/'))
}

fn default_cookie_path(path: &str) -> &str {
    match path.rfind('/') {
        Some(0) | None => "/",
        Some(index) => &path[..index],
    }
}
