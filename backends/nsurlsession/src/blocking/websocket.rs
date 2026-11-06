use std::borrow::Cow;
use std::sync::{mpsc, Arc};

use block2::RcBlock;
use nyquest_interface::websocket::{WebSocketError, WebSocketMessage, WebSocketResult};
use objc2::{AllocAnyThread, DefinedClass};
use objc2_foundation::{
    NSData, NSString, NSURLSessionWebSocketCloseCode, NSURLSessionWebSocketMessage,
};

use crate::blocking::waker::BlockingWaker;
use crate::datatask::GenericWaker;
use crate::websocket::{check_message_size, create_task, decode_received, into_ws_info, Socket};

impl Socket {
    pub(crate) fn send_blocking(&self, data: WebSocketMessage) -> WebSocketResult<()> {
        let message = match data {
            WebSocketMessage::Utf8(value) => NSURLSessionWebSocketMessage::initWithString(
                NSURLSessionWebSocketMessage::alloc(),
                &NSString::from_str(&value),
            ),
            WebSocketMessage::Bytes(value) => NSURLSessionWebSocketMessage::initWithData(
                NSURLSessionWebSocketMessage::alloc(),
                &NSData::from_vec(value),
            ),
            WebSocketMessage::Close { code, reason } => {
                let reason = NSData::from_vec(reason.into_bytes());
                self.task.cancelWithCloseCode_reason(
                    NSURLSessionWebSocketCloseCode(code as isize),
                    Some(&reason),
                );
                return Ok(());
            }
        };
        let (sender, receiver) = mpsc::channel();
        let callback = RcBlock::new(move |error: *mut objc2_foundation::NSError| {
            let result = if error.is_null() {
                Ok(())
            } else {
                Err(WebSocketError::Protocol("WebSocket send failed".into()))
            };
            let _ = sender.send(result);
        });
        unsafe {
            self.task.sendMessage_completionHandler(&message, &callback);
        }
        receiver
            .recv()
            .map_err(|_| WebSocketError::Protocol("WebSocket send callback stopped".into()))?
    }

    pub(crate) fn receive_blocking(&self) -> WebSocketResult<WebSocketMessage> {
        let (sender, receiver) = mpsc::channel();
        let task = self.task.clone();
        let callback = RcBlock::new(
            move |message: *mut NSURLSessionWebSocketMessage,
                  error: *mut objc2_foundation::NSError| {
                let _ = sender.send(decode_received(&task, message, error));
            },
        );
        unsafe {
            self.task.receiveMessageWithCompletionHandler(&callback);
        }
        let result = receiver
            .recv()
            .map_err(|_| WebSocketError::Protocol("WebSocket receive callback stopped".into()))??;
        check_message_size(result, self.max_message_size)
    }
}

pub(super) fn connect_blocking(
    client: &crate::client::NSUrlSessionClient,
    uri: Cow<'static, str>,
    protocols: Vec<Cow<'static, str>>,
) -> WebSocketResult<nyquest_interface::blocking::websocket::WebSocket> {
    let waker = Arc::new(BlockingWaker::new_from_current_thread());
    let (socket, delegate) = create_task(
        client,
        uri,
        protocols,
        GenericWaker::Blocking(waker.clone()),
    )?;
    let protocol = loop {
        if let Some(res) = delegate.ivars().result.lock().unwrap().take() {
            break res?;
        }
        waker.register_current_thread();
        if let Some(res) = delegate.ivars().result.lock().unwrap().take() {
            break res?;
        }
        std::thread::park();
    };
    Ok(nyquest_interface::blocking::websocket::WebSocket::new(
        into_ws_info(protocol),
        nyquest_interface::blocking::websocket::WebSocketSender::from_impl(Sender(socket.clone())),
        nyquest_interface::blocking::websocket::WebSocketReceiver::from_impl(Receiver(socket)),
    ))
}

struct Sender(Arc<Socket>);

impl nyquest_interface::blocking::websocket::BlockingWebSocketSender for Sender {
    fn send(&mut self, data: WebSocketMessage) -> WebSocketResult<()> {
        self.0.send_blocking(data)
    }

    fn close(&mut self) -> WebSocketResult<()> {
        self.send(WebSocketMessage::Close {
            code: 1000,
            reason: String::new(),
        })
    }
}

struct Receiver(Arc<Socket>);

impl nyquest_interface::blocking::websocket::BlockingWebSocketReceiver for Receiver {
    fn receive(&mut self) -> WebSocketResult<WebSocketMessage> {
        self.0.receive_blocking()
    }
}
