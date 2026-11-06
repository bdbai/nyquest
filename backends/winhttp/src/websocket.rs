use std::ptr::NonNull;

use nyquest_interface::websocket::{WebSocketError, WebSocketMessage, WebSocketResult};
use windows_sys::Win32::Networking::WinHttp::*;

use crate::error::WinHttpError;
use crate::url::concat_url;

pub(crate) const WEBSOCKET_RECV_BUFFER_SIZE: usize = 4 * 1024;

pub(crate) struct WebSocketHandle {
    raw: NonNull<std::ffi::c_void>,
}

unsafe impl Send for WebSocketHandle {}
unsafe impl Sync for WebSocketHandle {}

impl Drop for WebSocketHandle {
    fn drop(&mut self) {
        unsafe {
            WinHttpCloseHandle(self.raw.as_ptr());
        }
    }
}

impl WebSocketHandle {
    pub(crate) unsafe fn from_raw(raw: NonNull<std::ffi::c_void>) -> Self {
        Self { raw }
    }

    pub(crate) fn as_raw(&self) -> *mut std::ffi::c_void {
        self.raw.as_ptr()
    }

    pub(crate) fn query_close_status(&self) -> WebSocketResult<WebSocketMessage> {
        let mut code = WINHTTP_WEB_SOCKET_EMPTY_CLOSE_STATUS as u16;
        let mut reason = [0u8; WINHTTP_WEB_SOCKET_MAX_CLOSE_REASON_LENGTH as usize];
        let mut reason_len = 0;
        let result = unsafe {
            WinHttpWebSocketQueryCloseStatus(
                self.raw.as_ptr(),
                &mut code,
                reason.as_mut_ptr().cast(),
                reason.len() as u32,
                &mut reason_len,
            )
        };
        if result != 0 {
            return Err(winhttp_error(result, "WinHttpWebSocketQueryCloseStatus"));
        }
        Ok(WebSocketMessage::Close {
            code,
            reason: String::from_utf8_lossy(&reason[..reason_len as usize]).into_owned(),
        })
    }
}

pub(crate) fn winhttp_error(code: u32, context: &'static str) -> WebSocketError {
    from_winhttp_error(WinHttpError::from_code(code, context))
}

pub(crate) fn from_winhttp_error(err: WinHttpError) -> WebSocketError {
    WebSocketError::Io(std::io::Error::from(err))
}

pub(crate) fn prepare_websocket_url(
    base_cwurl: Option<&[u16]>,
    uri: &str,
) -> WebSocketResult<Vec<u16>> {
    if let Some((_, ws_url)) = uri
        .split_at_checked(5)
        .filter(|(p, _)| p.eq_ignore_ascii_case("ws://"))
    {
        let url = format!("http://{ws_url}");
        Ok(url.encode_utf16().collect())
    } else if let Some((_, wss_url)) = uri
        .split_at_checked(6)
        .filter(|(p, _)| p.eq_ignore_ascii_case("wss://"))
    {
        let url = format!("https://{wss_url}");
        Ok(url.encode_utf16().collect())
    } else {
        concat_url(base_cwurl, uri).map_err(|_| WebSocketError::InvalidUrl)
    }
}

pub(crate) fn build_websocket_headers(
    protocols: &[std::borrow::Cow<'static, str>],
    default_headers: &[(String, String)],
) -> String {
    let mut headers = String::new();
    if !protocols.is_empty() {
        let header_value = protocols.join(", ");
        headers = format!("Sec-WebSocket-Protocol: {header_value}\r\n");
    }
    for (name, value) in default_headers {
        headers.push_str(name);
        headers.push_str(": ");
        headers.push_str(value);
        headers.push_str("\r\n");
    }
    headers
}

pub(crate) fn query_negotiated_protocol(
    request: &crate::handle::RequestHandle,
) -> WebSocketResult<Option<String>> {
    Ok(request
        .query_header("Sec-WebSocket-Protocol")
        .map_err(from_winhttp_error)?
        .into_iter()
        .next()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prepare_websocket_url() {
        let inputs = [
            (None, "ws://example.com/aa", "http://example.com/aa"),
            (None, "wss://example.com/aa", "https://example.com/aa"),
            (
                Some("http://base.com/bb/"),
                "ws://example.com/aa",
                "http://example.com/aa",
            ),
            (
                Some("http://base.com/bb/"),
                "wss://example.com/aa",
                "https://example.com/aa",
            ),
            (
                Some("http://base.com/bb/"),
                "cc/dd",
                "http://base.com/bb/cc/dd",
            ),
        ];
        for (base, uri, expected) in inputs {
            let base_cwurl = base.map(|s| {
                s.encode_utf16()
                    .chain(std::iter::once(0))
                    .collect::<Vec<u16>>()
            });
            let result = prepare_websocket_url(base_cwurl.as_deref(), uri).unwrap();
            let result = String::from_utf16_lossy(&result);
            assert_eq!(result, expected, "Failed for base: {base:?}, uri: {uri}");
        }
    }

    #[test]
    fn test_build_websocket_headers() {
        let protocols = vec!["protocol1".into(), "protocol2".into()];
        let default_headers = vec![
            ("Header1".to_string(), "Value1".to_string()),
            ("Header2".to_string(), "Value2".to_string()),
        ];
        let headers = build_websocket_headers(&protocols, &default_headers);
        assert_eq!(headers, "Sec-WebSocket-Protocol: protocol1, protocol2\r\nHeader1: Value1\r\nHeader2: Value2\r\n");
    }
}
