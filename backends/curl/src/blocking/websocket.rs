use std::borrow::Cow;
use std::ffi::{c_int, c_void};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use nyquest_interface::websocket::{WebSocketError, WebSocketMessage, WebSocketResult};

use super::handler::BlockingHandler;
use crate::curl_ng::easy::{AsRawEasyMut, Share};
use crate::curl_ng::multi::RawMulti;
use crate::curl_ng::WithCurlCodeContext;
use crate::request::{create_easy, BoxEasyHandle};
use crate::websocket::{
    configure_websocket_easy, curl_error, curl_ws_recv, curl_ws_send, decode_close_payload,
    encode_message, nyquest_to_ws_error, CurlWsFrame, CURLWS_BINARY, CURLWS_CLOSE, CURLWS_CONT,
    CURLWS_PING, CURLWS_PONG, CURLWS_TEXT,
};

struct Connection {
    easy: BoxEasyHandle<BlockingHandler>,
    wait_multi: RawMulti,
    max_message_size: Option<usize>,
}

unsafe impl Send for Connection {}

impl Connection {
    fn raw(&mut self) -> *mut curl_sys::CURL {
        self.easy.as_mut().as_raw_easy_mut().raw()
    }

    fn connect(
        share: &Share,
        uri: Cow<'static, str>,
        protocol: Option<Cow<'static, str>>,
        options: &nyquest_interface::client::ClientOptions,
    ) -> WebSocketResult<Self> {
        let mut easy =
            create_easy(BlockingHandler::default(), share).map_err(nyquest_to_ws_error)?;
        configure_websocket_easy(easy.as_mut(), options, uri, protocol)?;
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

        let max_message_size = options
            .max_response_buffer_size
            .and_then(|size| usize::try_from(size).ok());

        Ok(Self {
            easy,
            wait_multi: RawMulti::new(),
            max_message_size,
        })
    }

    fn wait_socket(&mut self, for_recv: bool) {
        const CURLINFO_ACTIVESOCKET: curl_sys::CURLINFO = 0x500000 + 44;
        let mut sock: curl_sys::curl_socket_t = curl_sys::CURL_SOCKET_BAD;
        let code = unsafe {
            curl_sys::curl_easy_getinfo(
                self.raw(),
                CURLINFO_ACTIVESOCKET,
                &mut sock as *mut curl_sys::curl_socket_t,
            )
        };
        if code == curl_sys::CURLE_OK && sock != curl_sys::CURL_SOCKET_BAD {
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
            let mcode = unsafe {
                curl_sys::curl_multi_poll(self.wait_multi.raw(), &mut waitfd, 1, 100, &mut ret)
            };
            if mcode == curl_sys::CURLM_OK {
                return;
            }
        }
        std::thread::sleep(Duration::from_millis(1));
    }

    fn send(&mut self, data: WebSocketMessage) -> WebSocketResult<()> {
        let (flags, payload) = encode_message(data);
        self.send_frame(&payload, flags)
    }

    fn send_frame(&mut self, mut buffer: &[u8], flags: c_int) -> WebSocketResult<()> {
        loop {
            let mut sent = 0;
            let result = unsafe {
                curl_ws_send(
                    self.raw(),
                    buffer.as_ptr() as *const c_void,
                    buffer.len(),
                    &mut sent,
                    0,
                    flags,
                )
            };
            if result == curl_sys::CURLE_AGAIN {
                self.wait_socket(false);
                continue;
            }
            if result != curl_sys::CURLE_OK {
                return Err(curl_error(result));
            }
            if sent >= buffer.len() {
                return Ok(());
            }
            buffer = &buffer[sent..];
        }
    }

    fn receive(&mut self) -> WebSocketResult<WebSocketMessage> {
        let mut full_payload = Vec::new();
        let mut expected_type = None;
        let mut chunk = vec![0u8; 16 * 1024];

        loop {
            let mut received = 0;
            let mut meta_ptr: *const CurlWsFrame = std::ptr::null();
            let result = unsafe {
                curl_ws_recv(
                    self.raw(),
                    chunk.as_mut_ptr() as *mut c_void,
                    chunk.len(),
                    &mut received,
                    &mut meta_ptr,
                )
            };
            if result == curl_sys::CURLE_AGAIN {
                self.wait_socket(true);
                continue;
            }
            if result != curl_sys::CURLE_OK {
                return Err(curl_error(result));
            }

            let meta = if !meta_ptr.is_null() {
                unsafe { &*meta_ptr }
            } else {
                return Err(WebSocketError::Protocol("missing frame metadata".into()));
            };

            let flags = meta.flags;
            if flags & (CURLWS_PING | CURLWS_PONG) != 0 {
                continue;
            }

            if flags & CURLWS_CLOSE != 0 {
                full_payload.extend_from_slice(&chunk[..received]);
                if meta.bytesleft == 0 {
                    return Ok(decode_close_payload(&full_payload));
                }
                continue;
            }

            if let Some(limit) = self.max_message_size {
                if full_payload.len().saturating_add(received) > limit {
                    return Err(WebSocketError::MessageTooLarge);
                }
            }

            full_payload.extend_from_slice(&chunk[..received]);

            if expected_type.is_none() {
                expected_type = Some(flags & (CURLWS_TEXT | CURLWS_BINARY));
            }

            if meta.bytesleft == 0 && (flags & CURLWS_CONT == 0) {
                match expected_type {
                    Some(kind) if kind & CURLWS_TEXT != 0 => {
                        return String::from_utf8(full_payload)
                            .map(WebSocketMessage::Utf8)
                            .map_err(|_| WebSocketError::Protocol("invalid UTF-8".into()));
                    }
                    Some(kind) if kind & CURLWS_BINARY != 0 => {
                        return Ok(WebSocketMessage::Bytes(full_payload));
                    }
                    _ => {
                        return Err(WebSocketError::Protocol("unknown frame type".into()));
                    }
                }
            }
        }
    }
}

pub(super) fn connect_blocking(
    share: &Share,
    uri: Cow<'static, str>,
    protocol: Option<Cow<'static, str>>,
    options: &nyquest_interface::client::ClientOptions,
) -> WebSocketResult<nyquest_interface::blocking::websocket::WebSocket> {
    let connection = Arc::new(Mutex::new(Connection::connect(
        share, uri, protocol, options,
    )?));
    Ok(nyquest_interface::blocking::websocket::WebSocket::new(
        nyquest_interface::blocking::websocket::WebSocketSender::from_impl(BlockingSender {
            connection: connection.clone(),
        }),
        nyquest_interface::blocking::websocket::WebSocketReceiver::from_impl(BlockingReceiver {
            connection,
        }),
    ))
}

struct BlockingSender {
    connection: Arc<Mutex<Connection>>,
}

impl nyquest_interface::blocking::websocket::BlockingWebSocketSender for BlockingSender {
    fn send(&mut self, data: WebSocketMessage) -> WebSocketResult<()> {
        self.connection.lock().unwrap().send(data)
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
}

impl nyquest_interface::blocking::websocket::BlockingWebSocketReceiver for BlockingReceiver {
    fn receive(&mut self) -> WebSocketResult<WebSocketMessage> {
        self.connection.lock().unwrap().receive()
    }
}
