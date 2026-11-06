use std::borrow::Cow;
use std::ffi::{c_int, c_void};
use std::pin::Pin;

use nyquest_interface::client::ClientOptions;
use nyquest_interface::websocket::{WebSocketError, WebSocketMessage, WebSocketResult};

use crate::curl_ng::easy::{AsRawEasyMut, EasyCallback};
use crate::curl_ng::CurlStringList;
use crate::request::{populate_options, EasyHandle};
use crate::url::concat_url;

pub(crate) const CURLWS_TEXT: c_int = 1 << 0;
pub(crate) const CURLWS_BINARY: c_int = 1 << 1;
pub(crate) const CURLWS_CONT: c_int = 1 << 2;
pub(crate) const CURLWS_CLOSE: c_int = 1 << 3;
pub(crate) const CURLWS_PING: c_int = 1 << 4;
pub(crate) const CURLWS_PONG: c_int = 1 << 6;

#[repr(C)]
pub(crate) struct CurlWsFrame {
    pub age: c_int,
    pub flags: c_int,
    pub offset: curl_sys::curl_off_t,
    pub bytesleft: curl_sys::curl_off_t,
    pub len: usize,
}

unsafe extern "C" {
    #[cfg(feature = "blocking")]
    pub(crate) fn curl_ws_recv(
        curl: *mut curl_sys::CURL,
        buffer: *mut c_void,
        buflen: usize,
        received: *mut usize,
        meta: *mut *const CurlWsFrame,
    ) -> curl_sys::CURLcode;
    #[cfg(feature = "blocking")]
    pub(crate) fn curl_ws_send(
        curl: *mut curl_sys::CURL,
        buffer: *const c_void,
        buflen: usize,
        sent: *mut usize,
        fragsize: curl_sys::curl_off_t,
        flags: c_int,
    ) -> curl_sys::CURLcode;
    #[cfg(feature = "async")]
    pub(crate) fn curl_ws_meta(curl: *mut curl_sys::CURL) -> *const CurlWsFrame;
    #[cfg(feature = "async")]
    pub(crate) fn curl_ws_start_frame(
        curl: *mut curl_sys::CURL,
        flags: c_int,
        frame_len: curl_sys::curl_off_t,
    ) -> curl_sys::CURLcode;
}

pub(crate) fn curl_error(code: curl_sys::CURLcode) -> WebSocketError {
    WebSocketError::Io(std::io::Error::other(format!(
        "libcurl error {code}: {}",
        unsafe { std::ffi::CStr::from_ptr(curl_sys::curl_easy_strerror(code)).to_string_lossy() }
    )))
}

pub(crate) fn nyquest_to_ws_error(err: nyquest_interface::Error) -> WebSocketError {
    match err {
        nyquest_interface::Error::InvalidUrl => WebSocketError::InvalidUrl,
        nyquest_interface::Error::Io(e) => WebSocketError::Io(e),
        nyquest_interface::Error::ResponseTooLarge => WebSocketError::MessageTooLarge,
        nyquest_interface::Error::RequestTimeout => WebSocketError::Io(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            "request timed out",
        )),
    }
}

pub(crate) fn encode_message(data: WebSocketMessage) -> (c_int, Vec<u8>) {
    match data {
        WebSocketMessage::Utf8(s) => (CURLWS_TEXT, s.into_bytes()),
        WebSocketMessage::Bytes(b) => (CURLWS_BINARY, b),
        WebSocketMessage::Close { code, reason } => {
            let mut payload = Vec::with_capacity(2 + reason.len());
            payload.extend_from_slice(&code.to_be_bytes());
            payload.extend_from_slice(reason.as_bytes());
            (CURLWS_CLOSE, payload)
        }
    }
}

pub(crate) fn decode_close_payload(payload: &[u8]) -> WebSocketMessage {
    let (code, reason) = if payload.len() >= 2 {
        let code = u16::from_be_bytes([payload[0], payload[1]]);
        let reason = String::from_utf8_lossy(&payload[2..]).into_owned();
        (code, reason)
    } else {
        (1005, String::new())
    };
    WebSocketMessage::Close { code, reason }
}

pub(crate) fn extract_negotiated_protocol(headers: &[Vec<u8>]) -> Option<String> {
    headers
        .iter()
        .filter_map(|line| std::str::from_utf8(line).ok())
        .filter_map(|line| line.split_once(':'))
        .find(|(k, _)| k.trim().eq_ignore_ascii_case("Sec-WebSocket-Protocol"))
        .map(|(_, v)| v.trim())
        .filter(|v| !v.is_empty())
        .map(ToOwned::to_owned)
}

pub(crate) fn configure_websocket_easy<C: EasyCallback>(
    mut easy: Pin<&mut EasyHandle<C>>,
    options: &ClientOptions,
    uri: Cow<'static, str>,
    protocols: Vec<Cow<'static, str>>,
) -> WebSocketResult<()> {
    let url = concat_url(options.base_url.as_deref(), &uri);
    let is_ws = url
        .get(..5)
        .is_some_and(|s| s.eq_ignore_ascii_case("ws://"))
        || url
            .get(..6)
            .is_some_and(|s| s.eq_ignore_ascii_case("wss://"));
    if !is_ws || url.contains('\0') {
        return Err(WebSocketError::InvalidUrl);
    }

    let mut header_list = CurlStringList::default();
    if !protocols.is_empty() {
        let joined = protocols.join(", ");
        header_list.append(format!("Sec-WebSocket-Protocol: {joined}"));
    }
    for (name, value) in &options.default_headers {
        header_list.append(format!("{name}: {value}"));
    }

    easy.as_mut()
        .with_error_message(|mut e| {
            populate_options(&url, options, e.as_mut().as_raw_easy_mut())?;
            let mut raw = e.as_mut().as_raw_easy_mut();
            raw.as_mut().set_url(url)?;
            raw.as_mut().set_ws_options(0)?;
            let header = e.as_easy_mut().as_easy_mut().as_easy_mut();
            header.set_headers(Some(header_list))?;
            Ok(())
        })
        .map_err(|e| nyquest_to_ws_error(e.into()))
}
