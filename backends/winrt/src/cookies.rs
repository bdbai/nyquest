use std::time::{Duration, SystemTime};

use nyquest_interface::{Cookie, Result as NyquestResult};
use windows::core::HSTRING;
use windows::Web::Http::HttpCookie;

use crate::client::WinrtClient;
use crate::error::IntoNyquestResult;
use crate::uri::build_uri;

const WINDOWS_EPOCH_OFFSET: i64 = 116_444_736_000_000_000; // 100ns intervals from 1601 to 1970

impl WinrtClient {
    pub(crate) fn add_cookie_impl(&self, name: &str, value: &str, url: &str) -> NyquestResult<()> {
        let uri = build_uri(&self.base_url, url).into_nyquest_result()?;
        let domain = uri.Host().into_nyquest_result()?;
        let uri_path = uri.Path().into_nyquest_result()?.to_string();
        let path = default_cookie_path(&uri_path);

        let cookie = HttpCookie::Create(&HSTRING::from(name), &domain, &HSTRING::from(path))
            .into_nyquest_result()?;
        cookie
            .SetValue(&HSTRING::from(value))
            .into_nyquest_result()?;

        // The return value indicates whether an existing cookie was replaced, not whether the
        // cookie was accepted.
        self.cookie_manager
            .SetCookie(&cookie)
            .into_nyquest_result()?;
        Ok(())
    }

    pub(crate) fn get_cookies_for_impl(&self, url: &str) -> NyquestResult<Vec<Cookie>> {
        let uri = build_uri(&self.base_url, url).into_nyquest_result()?;
        let collection = self.cookie_manager.GetCookies(&uri).into_nyquest_result()?;

        let mut cookies = Vec::with_capacity(collection.Size().into_nyquest_result()? as usize);
        for index in 0..collection.Size().into_nyquest_result()? {
            let cookie = collection.GetAt(index).into_nyquest_result()?;
            cookies.push(cookie_to_nyquest(&cookie)?);
        }
        Ok(cookies)
    }
}

fn cookie_to_nyquest(cookie: &HttpCookie) -> NyquestResult<Cookie> {
    let expires = cookie
        .Expires()
        .ok()
        .and_then(|reference| reference.Value().ok())
        .and_then(|datetime| {
            let unix_ticks = datetime.UniversalTime.checked_sub(WINDOWS_EPOCH_OFFSET)?;
            let nanos = u64::try_from(unix_ticks).ok()?.checked_mul(100)?;
            Some(SystemTime::UNIX_EPOCH + Duration::from_nanos(nanos))
        });

    Ok(Cookie {
        name: cookie.Name().into_nyquest_result()?.to_string(),
        value: cookie.Value().into_nyquest_result()?.to_string(),
        domain: cookie
            .Domain()
            .into_nyquest_result()?
            .to_string()
            .trim_start_matches('.')
            .to_owned(),
        path: cookie.Path().into_nyquest_result()?.to_string(),
        expires,
        is_http_only: cookie.HttpOnly().into_nyquest_result()?,
        is_secure: cookie.Secure().into_nyquest_result()?,
    })
}

fn default_cookie_path(path: &str) -> &str {
    match path.rfind('/') {
        Some(0) | None => "/",
        Some(index) => &path[..index],
    }
}
