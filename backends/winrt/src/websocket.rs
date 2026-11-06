use std::borrow::Cow;

use nyquest_interface::websocket::{WebSocketError, WebSocketMessage, WebSocketResult};
use windows::core::HSTRING;
use windows::Foundation::{TypedEventHandler, Uri};
use windows::Networking::Sockets::{
    IWebSocket, MessageWebSocket, MessageWebSocketMessageReceivedEventArgs,
    MessageWebSocketReceiveMode, SocketMessageType, WebSocketClosedEventArgs,
};
use windows::Storage::Streams::{
    DataReader, DataWriter, DataWriterStoreOperation, InputStreamOptions,
};

use crate::client::WinrtClient;
use crate::error::IntoNyquestResult;
use crate::uri::build_uri;

pub(crate) enum Received {
    Message {
        message_type: SocketMessageType,
        reader: DataReader,
        complete: bool,
    },
    Closed {
        code: u16,
        reason: String,
    },
    Error(WebSocketError),
}

pub(crate) fn prepare_send_message(
    websocket: &MessageWebSocket,
    data: WebSocketMessage,
) -> WebSocketResult<Option<(DataWriter, DataWriterStoreOperation)>> {
    let (kind, text, bytes) = match data {
        WebSocketMessage::Utf8(value) => (SocketMessageType::Utf8, Some(value), None),
        WebSocketMessage::Bytes(value) => (SocketMessageType::Binary, None, Some(value)),
        WebSocketMessage::Close { code, reason } => {
            websocket
                .CloseWithStatus(code, &HSTRING::from(reason))
                .into_nyquest_result()
                .map_err(ws_error)?;
            return Ok(None);
        }
    };
    let control = websocket
        .Control()
        .into_nyquest_result()
        .map_err(|error| ws_error_context("control", error))?;
    control
        .SetMessageType(kind)
        .into_nyquest_result()
        .map_err(|error| ws_error_context("message type", error))?;
    let output = websocket
        .OutputStream()
        .into_nyquest_result()
        .map_err(|error| ws_error_context("output stream", error))?;
    let writer = DataWriter::CreateDataWriter(&output)
        .into_nyquest_result()
        .map_err(|error| ws_error_context("writer", error))?;
    if let Some(text) = text {
        writer
            .WriteString(&HSTRING::from(text))
            .into_nyquest_result()
            .map_err(|error| ws_error_context("write", error))?;
    } else if let Some(bytes) = bytes {
        writer
            .WriteBytes(&bytes)
            .into_nyquest_result()
            .map_err(|error| ws_error_context("write", error))?;
    }
    let store_op = writer
        .StoreAsync()
        .into_nyquest_result()
        .map_err(|error| ws_error_context("store", error))?;
    Ok(Some((writer, store_op)))
}

pub(crate) fn detach_writer(writer: DataWriter) -> WebSocketResult<()> {
    writer
        .DetachStream()
        .into_nyquest_result()
        .map_err(|error| ws_error_context("detach stream", error))?;
    Ok(())
}

/// Reads data from a DataReader fragment and appends to the buffer.
pub(crate) fn read_fragment(
    reader: &DataReader,
    bytes: &mut Vec<u8>,
    max_size: Option<usize>,
) -> WebSocketResult<()> {
    let count = reader
        .UnconsumedBufferLength()
        .into_nyquest_result()
        .map_err(|error| ws_error_context("length", error))? as usize;
    if max_size.is_some_and(|limit| bytes.len().saturating_add(count) > limit) {
        return Err(WebSocketError::MessageTooLarge);
    }
    if count != 0 {
        let offset = bytes.len();
        bytes.resize(offset + count, 0);
        reader
            .ReadBytes(&mut bytes[offset..])
            .into_nyquest_result()
            .map_err(|error| ws_error_context("read", error))?;
    }
    Ok(())
}

pub(crate) fn finalize_message(
    message_type: SocketMessageType,
    bytes: Vec<u8>,
) -> WebSocketResult<WebSocketMessage> {
    if message_type == SocketMessageType::Utf8 {
        String::from_utf8(bytes)
            .map(WebSocketMessage::Utf8)
            .map_err(|_| WebSocketError::Protocol("invalid UTF-8 message".into()))
    } else {
        Ok(WebSocketMessage::Bytes(bytes))
    }
}

pub(crate) fn ws_error(error: impl std::fmt::Display + std::fmt::Debug) -> WebSocketError {
    WebSocketError::Protocol(format!("{error:?}"))
}

pub(crate) fn ws_error_context(
    context: &str,
    error: impl std::fmt::Display + std::fmt::Debug,
) -> WebSocketError {
    WebSocketError::Protocol(format!("{context}: {error:?}"))
}

pub(crate) fn extract_negotiated_protocol(
    websocket: &MessageWebSocket,
) -> WebSocketResult<Option<String>> {
    let protocol = websocket
        .Information()
        .into_nyquest_result()
        .map_err(ws_error)?
        .Protocol()
        .into_nyquest_result()
        .map_err(ws_error)?
        .to_string_lossy();
    Ok((!protocol.is_empty()).then_some(protocol))
}

impl WinrtClient {
    pub(crate) fn validate_ws_uri(&self, uri: &str) -> WebSocketResult<Uri> {
        let uri_parsed = build_uri(&self.base_url, uri).map_err(|_| WebSocketError::InvalidUrl)?;
        let scheme = uri_parsed
            .SchemeName()
            .map_err(|_| WebSocketError::InvalidUrl)?
            .to_string_lossy();
        if !scheme.eq_ignore_ascii_case("ws") && !scheme.eq_ignore_ascii_case("wss") {
            return Err(WebSocketError::InvalidUrl);
        }
        Ok(uri_parsed)
    }

    pub(crate) fn prepare_ws(
        &self,
        ws: &MessageWebSocket,
        protocols: Vec<Cow<'static, str>>,
        on_event: impl Fn(Received) + Send + 'static,
        on_close: impl Fn(Received) + Send + 'static,
    ) -> WebSocketResult<()> {
        let control = ws.Control().into_nyquest_result().map_err(ws_error)?;
        control
            .SetReceiveMode(MessageWebSocketReceiveMode::PartialMessage)
            .into_nyquest_result()
            .map_err(ws_error)?;
        if !protocols.is_empty() {
            let supported = control
                .SupportedProtocols()
                .into_nyquest_result()
                .map_err(ws_error)?;
            for protocol in protocols {
                supported
                    .Append(&HSTRING::from(protocol.as_ref()))
                    .into_nyquest_result()
                    .map_err(ws_error)?;
            }
        }

        let msg_handler = TypedEventHandler::<
            MessageWebSocket,
            MessageWebSocketMessageReceivedEventArgs,
        >::new(move |_socket, args| {
            let Some(args) = args.as_ref() else {
                return Ok(());
            };
            let item = match extract_received_message(args) {
                Ok(msg) => msg,
                Err(err) => Received::Error(ws_error(err)),
            };
            on_event(item);
            Ok(())
        });

        let close_handler =
            TypedEventHandler::<IWebSocket, WebSocketClosedEventArgs>::new(move |_socket, args| {
                let Some(args) = args.as_ref() else {
                    return Ok(());
                };
                let item = match (args.Code(), args.Reason()) {
                    (Ok(code), Ok(reason)) => Received::Closed {
                        code,
                        reason: reason.to_string(),
                    },
                    (Err(err), _) | (_, Err(err)) => Received::Error(ws_error(err)),
                };
                on_close(item);
                Ok(())
            });

        ws.Closed(&close_handler)
            .into_nyquest_result()
            .map_err(ws_error)?;
        ws.MessageReceived(&msg_handler)
            .into_nyquest_result()
            .map_err(ws_error)?;

        Ok(())
    }
}

fn extract_received_message(
    args: &MessageWebSocketMessageReceivedEventArgs,
) -> windows::core::Result<Received> {
    let reader = args.GetDataReader()?;
    reader.SetInputStreamOptions(InputStreamOptions::Partial)?;
    Ok(Received::Message {
        message_type: args.MessageType()?,
        reader,
        complete: args.IsMessageComplete()?,
    })
}
