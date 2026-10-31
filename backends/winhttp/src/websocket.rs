use std::ptr::NonNull;

use nyquest_interface::websocket::{WebSocketError, WebSocketMessage, WebSocketResult};
use windows_sys::Win32::Networking::WinHttp::*;

use crate::error::WinHttpError;
use crate::url::concat_url;

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
    pub(crate) fn from_raw(raw: NonNull<std::ffi::c_void>) -> Self {
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
    let combined = concat_url(base_cwurl, uri).map_err(|_| WebSocketError::InvalidUrl)?;
    // Check for ws:// or wss:// prefix and convert to http:// or https:// for WinHttpCrackUrl
    let ws_prefix: [u16; 5] = [
        b'w' as u16,
        b's' as u16,
        b':' as u16,
        b'/' as u16,
        b'/' as u16,
    ];
    let wss_prefix: [u16; 6] = [
        b'w' as u16,
        b's' as u16,
        b's' as u16,
        b':' as u16,
        b'/' as u16,
        b'/' as u16,
    ];
    let http_prefix: [u16; 7] = [
        b'h' as u16,
        b't' as u16,
        b't' as u16,
        b'p' as u16,
        b':' as u16,
        b'/' as u16,
        b'/' as u16,
    ];
    let https_prefix: [u16; 8] = [
        b'h' as u16,
        b't' as u16,
        b't' as u16,
        b'p' as u16,
        b's' as u16,
        b':' as u16,
        b'/' as u16,
        b'/' as u16,
    ];

    let mut out = if combined.len() >= 5 && eq_ignore_ascii_case_u16(&combined[..5], &ws_prefix) {
        let mut v = Vec::with_capacity(combined.len() + 3);
        v.extend_from_slice(&http_prefix);
        v.extend_from_slice(&combined[5..]);
        v
    } else if combined.len() >= 6 && eq_ignore_ascii_case_u16(&combined[..6], &wss_prefix) {
        let mut v = Vec::with_capacity(combined.len() + 3);
        v.extend_from_slice(&https_prefix);
        v.extend_from_slice(&combined[6..]);
        v
    } else {
        return Err(WebSocketError::InvalidUrl);
    };
    out.push(0);
    Ok(out)
}

fn eq_ignore_ascii_case_u16(a: &[u16], b: &[u16]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b.iter()).all(|(&x, &y)| {
            u8::try_from(x)
                .ok()
                .zip(u8::try_from(y).ok())
                .is_some_and(|(cx, cy)| cx.eq_ignore_ascii_case(&cy))
        })
}

pub(crate) fn build_websocket_headers(
    protocol: Option<&str>,
    default_headers: &[(String, String)],
) -> String {
    let mut headers = String::new();
    if let Some(protocol) = protocol {
        headers.push_str("Sec-WebSocket-Protocol: ");
        headers.push_str(protocol);
        headers.push_str("\r\n");
    }
    for (name, value) in default_headers {
        headers.push_str(name);
        headers.push_str(": ");
        headers.push_str(value);
        headers.push_str("\r\n");
    }
    headers
}
