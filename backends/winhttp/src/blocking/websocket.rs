use std::borrow::Cow;
use std::sync::Arc;

use nyquest_interface::websocket::{WebSocketError, WebSocketMessage, WebSocketResult};
use windows_sys::Win32::Networking::WinHttp::*;

use crate::handle::ConnectionHandle;
use crate::request::{create_request, method_to_cwstr};
use crate::session::WinHttpSession;
use crate::url::ParsedUrl;
use crate::websocket::{
    build_websocket_headers, from_winhttp_error, prepare_websocket_url, winhttp_error,
    WebSocketHandle,
};

struct BlockingSocket {
    _session: Arc<WinHttpSession>,
    _connection: ConnectionHandle,
    handle: WebSocketHandle,
}

impl BlockingSocket {
    fn send(&self, data: WebSocketMessage) -> WebSocketResult<()> {
        match data {
            WebSocketMessage::Utf8(value) => {
                let bytes = value.into_bytes();
                let ptr = if bytes.is_empty() {
                    std::ptr::null()
                } else {
                    bytes.as_ptr().cast()
                };
                let result = unsafe {
                    WinHttpWebSocketSend(
                        self.handle.as_raw(),
                        WINHTTP_WEB_SOCKET_UTF8_MESSAGE_BUFFER_TYPE,
                        ptr,
                        bytes.len() as u32,
                    )
                };
                if result == 0 {
                    Ok(())
                } else {
                    Err(winhttp_error(result, "WinHttpWebSocketSend"))
                }
            }
            WebSocketMessage::Bytes(value) => {
                let ptr = if value.is_empty() {
                    std::ptr::null()
                } else {
                    value.as_ptr().cast()
                };
                let result = unsafe {
                    WinHttpWebSocketSend(
                        self.handle.as_raw(),
                        WINHTTP_WEB_SOCKET_BINARY_MESSAGE_BUFFER_TYPE,
                        ptr,
                        value.len() as u32,
                    )
                };
                if result == 0 {
                    Ok(())
                } else {
                    Err(winhttp_error(result, "WinHttpWebSocketSend"))
                }
            }
            WebSocketMessage::Close { code, reason } => {
                let (reason_ptr, reason_len) = if reason.is_empty() {
                    (std::ptr::null(), 0)
                } else {
                    (reason.as_ptr().cast(), reason.len() as u32)
                };
                let result = unsafe {
                    WinHttpWebSocketShutdown(self.handle.as_raw(), code, reason_ptr, reason_len)
                };
                if result == 0 {
                    Ok(())
                } else {
                    Err(winhttp_error(result, "WinHttpWebSocketShutdown"))
                }
            }
        }
    }

    fn receive(&self, max_size: Option<u64>) -> WebSocketResult<WebSocketMessage> {
        let mut message = Vec::new();
        let mut buffer = vec![0u8; 16 * 1024];
        loop {
            let mut read = 0;
            let mut buffer_type = 0;
            let result = unsafe {
                WinHttpWebSocketReceive(
                    self.handle.as_raw(),
                    buffer.as_mut_ptr().cast(),
                    buffer.len() as u32,
                    &mut read,
                    &mut buffer_type,
                )
            };
            if result != 0 {
                return Err(winhttp_error(result, "WinHttpWebSocketReceive"));
            }
            let read_len = read as usize;
            if max_size.is_some_and(|limit| message.len().saturating_add(read_len) > limit as usize)
            {
                return Err(WebSocketError::MessageTooLarge);
            }
            message.extend_from_slice(&buffer[..read_len]);
            match buffer_type {
                WINHTTP_WEB_SOCKET_UTF8_MESSAGE_BUFFER_TYPE => {
                    return String::from_utf8(message)
                        .map(WebSocketMessage::Utf8)
                        .map_err(|_| WebSocketError::Protocol("invalid UTF-8 message".into()));
                }
                WINHTTP_WEB_SOCKET_BINARY_MESSAGE_BUFFER_TYPE => {
                    return Ok(WebSocketMessage::Bytes(message));
                }
                WINHTTP_WEB_SOCKET_CLOSE_BUFFER_TYPE => {
                    return self.handle.query_close_status();
                }
                WINHTTP_WEB_SOCKET_UTF8_FRAGMENT_BUFFER_TYPE
                | WINHTTP_WEB_SOCKET_BINARY_FRAGMENT_BUFFER_TYPE => {}
                _ => {
                    return Err(WebSocketError::Protocol(
                        "unknown WebSocket buffer type".into(),
                    ))
                }
            }
        }
    }
}

pub(super) fn connect_blocking(
    session: &Arc<WinHttpSession>,
    uri: Cow<'static, str>,
    protocol: Option<Cow<'static, str>>,
) -> WebSocketResult<nyquest_interface::blocking::websocket::WebSocket> {
    let url_wide = prepare_websocket_url(session.base_cwurl.as_deref(), &uri)?;
    let parsed_url = ParsedUrl::parse(&url_wide).ok_or(WebSocketError::InvalidUrl)?;
    let method = method_to_cwstr(&nyquest_interface::Method::Get);
    let (connection, request) =
        create_request(session, &parsed_url, &method).map_err(from_winhttp_error)?;

    let headers = build_websocket_headers(protocol.as_deref(), &session.options.default_headers);
    if !headers.is_empty() {
        request.add_headers(&headers).map_err(from_winhttp_error)?;
    }

    request
        .enable_websocket_upgrade()
        .map_err(from_winhttp_error)?;
    unsafe { request.send(std::ptr::null(), 0, 0) }.map_err(from_winhttp_error)?;
    request.receive_response().map_err(from_winhttp_error)?;

    let status = request.query_status_code().map_err(from_winhttp_error)?;
    if status != 101 {
        return Err(WebSocketError::Protocol(format!(
            "WebSocket upgrade failed: HTTP status {status}"
        )));
    }

    let raw = request
        .complete_websocket_upgrade(0)
        .map_err(from_winhttp_error)?;
    drop(request);

    let socket = Arc::new(BlockingSocket {
        _session: session.clone(),
        _connection: connection,
        handle: WebSocketHandle::from_raw(raw),
    });

    Ok(nyquest_interface::blocking::websocket::WebSocket::new(
        nyquest_interface::blocking::websocket::WebSocketSender::from_impl(Sender {
            socket: socket.clone(),
        }),
        nyquest_interface::blocking::websocket::WebSocketReceiver::from_impl(Receiver {
            socket,
            max_size: session.options.max_response_buffer_size,
        }),
    ))
}

struct Sender {
    socket: Arc<BlockingSocket>,
}

impl nyquest_interface::blocking::websocket::BlockingWebSocketSender for Sender {
    fn send(&mut self, data: WebSocketMessage) -> WebSocketResult<()> {
        self.socket.send(data)
    }

    fn close(&mut self) -> WebSocketResult<()> {
        self.send(WebSocketMessage::Close {
            code: 1000,
            reason: String::new(),
        })
    }
}

struct Receiver {
    socket: Arc<BlockingSocket>,
    max_size: Option<u64>,
}

impl nyquest_interface::blocking::websocket::BlockingWebSocketReceiver for Receiver {
    fn receive(&mut self) -> WebSocketResult<WebSocketMessage> {
        self.socket.receive(self.max_size)
    }
}
