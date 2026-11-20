use std::borrow::Cow;
use std::ffi::{c_int, c_void};
use std::io;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use nyquest_interface::websocket::{
    WebSocketError, WebSocketInfo, WebSocketMessage, WebSocketResult,
};

use super::handler::BlockingHandler;
use crate::curl_ng::easy::{AsRawEasyMut, Share};
use crate::curl_ng::multi::RawMulti;
use crate::curl_ng::url::CurlUrl;
use crate::curl_ng::WithCurlCodeContext;
use crate::request::{create_easy, AsCallbackMut, BoxEasyHandle};
use crate::websocket::{
    configure_websocket_easy, curl_error, curl_ws_recv, curl_ws_send, decode_close_payload,
    encode_message, extract_negotiated_protocol, nyquest_to_ws_error, CurlWsFrame, CURLWS_BINARY,
    CURLWS_CLOSE, CURLWS_CONT, CURLWS_PING, CURLWS_PONG, CURLWS_TEXT,
};

struct Connection {
    easy: BoxEasyHandle<BlockingHandler>,
}

unsafe impl Send for Connection {}

fn wait_socket(
    wait_multi: &mut RawMulti,
    sock: curl_sys::curl_socket_t,
    for_recv: bool,
) -> WebSocketResult<()> {
    let mut waitfd = curl_sys::curl_waitfd {
        fd: sock,
        events: if for_recv {
            curl_sys::CURL_WAIT_POLLIN
        } else {
            curl_sys::CURL_WAIT_POLLOUT
        },
        revents: 0,
    };
    let mut ret = 0;
    let mcode =
        unsafe { curl_sys::curl_multi_poll(wait_multi.raw(), &mut waitfd, 1, 1000, &mut ret) };
    if mcode == curl_sys::CURLM_OK {
        return Ok(());
    }
    std::thread::sleep(Duration::from_secs(1));
    Ok(())
}

impl Connection {
    fn raw(&mut self) -> *mut curl_sys::CURL {
        self.easy.as_mut().as_raw_easy_mut().raw()
    }

    fn active_socket(&mut self) -> WebSocketResult<curl_sys::curl_socket_t> {
        const CURLINFO_ACTIVESOCKET: curl_sys::CURLINFO = 0x500000 + 44;
        let mut sock: curl_sys::curl_socket_t = curl_sys::CURL_SOCKET_BAD;
        let code = unsafe {
            curl_sys::curl_easy_getinfo(
                self.raw(),
                CURLINFO_ACTIVESOCKET,
                &mut sock as *mut curl_sys::curl_socket_t,
            )
        };
        if code != curl_sys::CURLE_OK {
            return Err(curl_error(code));
        }
        if sock == curl_sys::CURL_SOCKET_BAD {
            return Err(WebSocketError::Io(io::Error::other(
                "WebSocket socket is invalid",
            )));
        }
        Ok(sock)
    }

    fn connect(
        share: &Share,
        base_url: Option<Arc<CurlUrl>>,
        uri: Cow<'static, str>,
        protocols: Vec<Cow<'static, str>>,
        options: &nyquest_interface::client::ClientOptions,
    ) -> WebSocketResult<(Self, Option<String>)> {
        let mut easy =
            create_easy(BlockingHandler::default(), share).map_err(nyquest_to_ws_error)?;
        configure_websocket_easy(easy.as_mut(), base_url.as_deref(), options, uri, protocols)?;
        easy.as_mut()
            .with_error_message(|mut e| {
                e.as_mut().as_raw_easy_mut().set_connect_only_websocket()?;
                unsafe {
                    curl_sys::curl_easy_perform(e.as_raw_easy_mut().raw())
                        .with_easy_context("curl_easy_perform websocket")
                }
            })
            .map_err(|e| nyquest_to_ws_error(e.into()))?;

        let status = easy
            .as_mut()
            .with_error_message(|e| e.as_raw_easy_mut().get_response_code())
            .map_err(|e| nyquest_to_ws_error(e.into()))?;
        if status != 101 {
            return Err(WebSocketError::Protocol(format!(
                "WebSocket upgrade failed: HTTP status {status}"
            )));
        }

        let protocol = extract_negotiated_protocol(&easy.as_callback_mut().state.get_headers());

        Ok((Self { easy }, protocol))
    }
}

pub(super) fn connect_blocking(
    share: &Share,
    base_url: Option<Arc<CurlUrl>>,
    uri: Cow<'static, str>,
    protocols: Vec<Cow<'static, str>>,
    options: &nyquest_interface::client::ClientOptions,
) -> WebSocketResult<nyquest_interface::blocking::websocket::WebSocket> {
    let (connection, protocol) = Connection::connect(share, base_url, uri, protocols, options)?;
    let connection = Arc::new(Mutex::new(connection));
    let max_message_size = options
        .max_response_buffer_size
        .and_then(|size| usize::try_from(size).ok());
    Ok(nyquest_interface::blocking::websocket::WebSocket::new(
        WebSocketInfo { protocol },
        nyquest_interface::blocking::websocket::WebSocketSender::from_impl(BlockingSender {
            connection: connection.clone(),
            wait_multi: RawMulti::new(),
        }),
        nyquest_interface::blocking::websocket::WebSocketReceiver::from_impl(BlockingReceiver {
            connection,
            wait_multi: RawMulti::new(),
            max_message_size,
        }),
    ))
}

struct BlockingSender {
    connection: Arc<Mutex<Connection>>,
    wait_multi: RawMulti,
}

unsafe impl Send for BlockingSender {}

impl BlockingSender {
    fn send_frame(&mut self, mut buffer: &[u8], flags: c_int) -> WebSocketResult<()> {
        loop {
            let mut sent = 0;
            let wait_sock = {
                let mut conn = self.connection.lock().unwrap();
                let result = unsafe {
                    curl_ws_send(
                        conn.raw(),
                        buffer.as_ptr() as *const c_void,
                        buffer.len(),
                        &mut sent,
                        0,
                        flags,
                    )
                };
                if result == curl_sys::CURLE_AGAIN {
                    Some(conn.active_socket()?)
                } else if result != curl_sys::CURLE_OK {
                    return Err(curl_error(result));
                } else {
                    None
                }
            };
            if let Some(sock) = wait_sock {
                wait_socket(&mut self.wait_multi, sock, false)?;
                continue;
            }
            if sent >= buffer.len() {
                return Ok(());
            }
            buffer = &buffer[sent..];
        }
    }
}

impl nyquest_interface::blocking::websocket::BlockingWebSocketSender for BlockingSender {
    fn send(&mut self, data: WebSocketMessage) -> WebSocketResult<()> {
        let (flags, payload) = encode_message(data);
        self.send_frame(&payload, flags)
    }

    fn close(&mut self) -> WebSocketResult<()> {
        self.send(WebSocketMessage::Close {
            code: 1000,
            reason: String::new(),
        })
    }
}

struct BlockingReceiver {
    connection: Arc<Mutex<Connection>>,
    wait_multi: RawMulti,
    max_message_size: Option<usize>,
}

unsafe impl Send for BlockingReceiver {}

impl nyquest_interface::blocking::websocket::BlockingWebSocketReceiver for BlockingReceiver {
    fn receive(&mut self) -> WebSocketResult<WebSocketMessage> {
        let mut full_payload = vec![];
        let mut frame_payload = vec![];
        let mut is_frame_text = false;

        loop {
            let mut received = 0;
            let recv_step = {
                let mut conn = self.connection.lock().unwrap();
                let mut meta_ptr: *const CurlWsFrame = std::ptr::null();
                let result = unsafe {
                    let buffer = frame_payload.spare_capacity_mut();
                    let ptr = if buffer.is_empty() {
                        std::ptr::null_mut()
                    } else {
                        buffer.as_mut_ptr() as *mut c_void
                    };
                    curl_ws_recv(conn.raw(), ptr, buffer.len(), &mut received, &mut meta_ptr)
                };
                match result {
                    curl_sys::CURLE_OK => {
                        if meta_ptr.is_null() {
                            return Err(WebSocketError::Protocol("missing frame metadata".into()));
                        }
                        Ok(unsafe { *meta_ptr })
                    }
                    curl_sys::CURLE_AGAIN => Err(conn.active_socket()?),
                    _ => return Err(curl_error(result)),
                }
            };
            let meta = match recv_step {
                Ok(meta) => meta,
                Err(sock) => {
                    wait_socket(&mut self.wait_multi, sock, true)?;
                    continue;
                }
            };

            let flags = meta.flags;
            if flags & (CURLWS_PING | CURLWS_PONG) != 0 {
                continue;
            }

            if flags & CURLWS_TEXT != 0 {
                is_frame_text = true;
            } else if flags & CURLWS_BINARY != 0 {
                is_frame_text = false;
            }

            let frame_remaining = meta.bytesleft.try_into().unwrap_or(usize::MAX);
            if let Some(limit) = self.max_message_size {
                let total_len = full_payload
                    .len()
                    .saturating_add(frame_payload.len())
                    .saturating_add(received)
                    .saturating_add(frame_remaining);
                if total_len > limit {
                    return Err(WebSocketError::MessageTooLarge);
                }
            }

            unsafe {
                frame_payload.set_len(frame_payload.len() + received);
            }

            if meta.bytesleft > 0 {
                frame_payload.reserve(meta.bytesleft.try_into().unwrap_or(usize::MAX));
                continue;
            }
            if flags & CURLWS_CLOSE != 0 {
                return Ok(decode_close_payload(&frame_payload));
            }

            // At this point, we have a complete frame in `frame_payload`. Append it to `full_payload` and check if it's the last frame of the message.
            full_payload.extend_from_slice(&frame_payload);
            if flags & CURLWS_CONT == 0 {
                if is_frame_text {
                    return String::from_utf8(full_payload)
                        .map(WebSocketMessage::Utf8)
                        .map_err(|_| WebSocketError::Protocol("invalid UTF-8".into()));
                } else {
                    return Ok(WebSocketMessage::Bytes(full_payload));
                }
            } else {
                // If it's a continuation frame, we need to receive the next frame.
                frame_payload.clear();
                continue;
            }
        }
    }
}
