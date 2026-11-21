use nyquest_interface::{Cookie, Error as NyquestError, Result as NyquestResult};
use url::Url;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::{CookieListItem, CookieStore, HtmlDocument};

use crate::client::ReqwestClient;
use crate::cookie_util::{decode_cookie_value, encode_cookie_value};

impl ReqwestClient {
    pub(crate) async fn add_cookie(
        &self,
        name: &str,
        value: &str,
        _url: &str,
    ) -> NyquestResult<()> {
        let Some(store) = self.cookie_store()? else {
            return Ok(());
        };
        JsFuture::from(store.set_with_name_and_value(name, value))
            .await
            .map_err(js_error)?;
        Ok(())
    }

    pub(crate) async fn get_cookies_for(&self, url: &str) -> NyquestResult<Vec<Cookie>> {
        let Some(store) = self.cookie_store()? else {
            return Ok(Vec::new());
        };
        let cookies = JsFuture::from(store.get_all()).await.map_err(js_error)?;
        let cookies: js_sys::Array = cookies
            .dyn_into()
            .map_err(|_| invalid_response("cookie list"))?;

        let (domain, path) = cookie_scope(url)?;
        Ok(cookies
            .iter()
            .map(|value| value.unchecked_into::<CookieListItem>())
            .filter_map(|item| {
                Some(Cookie {
                    name: item.get_name()?,
                    value: item.get_value()?,
                    domain: domain.clone(),
                    path: path.clone(),
                    expires: None,
                    is_http_only: false,
                    is_secure: false,
                })
            })
            .collect())
    }

    /// Adds a cookie through the synchronous `document.cookie` API.
    ///
    /// This is the blocking counterpart of [`Self::add_cookie`]. Since `document.cookie` is
    /// scoped to the current document, `url` cannot be used to target a different origin; it is
    /// ignored, just like the asynchronous Cookie Store API implementation.
    pub(crate) fn add_cookie_sync(&self, name: &str, value: &str, _url: &str) -> NyquestResult<()> {
        let Some(document) = self.document()? else {
            return Ok(());
        };
        document
            .set_cookie(&format!("{name}={}", encode_cookie_value(value)))
            .map_err(js_error)?;
        Ok(())
    }

    /// Returns the cookies exposed by the synchronous `document.cookie` API.
    ///
    /// `document.cookie` only exposes cookies for the current document and omits their
    /// attributes, so `url` is used solely to fill in the `domain` and `path` fields. The
    /// returned cookies always report no expiration, and are neither HttpOnly nor Secure.
    pub(crate) fn get_cookies_for_sync(&self, url: &str) -> NyquestResult<Vec<Cookie>> {
        let Some(document) = self.document()? else {
            return Ok(Vec::new());
        };
        let raw = document.cookie().map_err(js_error)?;
        let (domain, path) = cookie_scope(url)?;
        Ok(raw
            .split(';')
            .filter_map(|pair| {
                let (name, value) = pair.trim().split_once('=')?;
                Some(Cookie {
                    name: name.to_owned(),
                    value: decode_cookie_value(value),
                    domain: domain.clone(),
                    path: path.clone(),
                    expires: None,
                    is_http_only: false,
                    is_secure: false,
                })
            })
            .collect())
    }

    fn cookie_store(&self) -> NyquestResult<Option<CookieStore>> {
        if !self.wasm_options.use_cookies {
            return Ok(None);
        }
        let window = web_sys::window()
            .ok_or_else(|| unsupported("the Cookie Store API is not available"))?;
        Ok(Some(window.cookie_store()))
    }

    fn document(&self) -> NyquestResult<Option<HtmlDocument>> {
        if !self.wasm_options.use_cookies {
            return Ok(None);
        }
        let document = web_sys::window()
            .and_then(|window| window.document())
            .ok_or_else(|| unsupported("document.cookie is not available"))?;
        document
            .dyn_into::<HtmlDocument>()
            .map(Some)
            .map_err(|_| unsupported("document.cookie is not available"))
    }
}

fn cookie_scope(url: &str) -> NyquestResult<(String, String)> {
    let url = Url::parse(url).map_err(|_| NyquestError::InvalidUrl)?;
    let domain = url.host_str().unwrap_or_default().to_owned();
    let path = match url.path().rfind('/') {
        Some(0) | None => "/",
        Some(index) => &url.path()[..index],
    }
    .to_owned();
    Ok((domain, path))
}

fn js_error(value: JsValue) -> NyquestError {
    unsupported(&format!("cookie API error: {value:?}"))
}

fn invalid_response(what: &str) -> NyquestError {
    unsupported(&format!("unexpected Cookie Store API response: {what}"))
}

fn unsupported(message: &str) -> NyquestError {
    NyquestError::Io(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        message.to_owned(),
    ))
}
