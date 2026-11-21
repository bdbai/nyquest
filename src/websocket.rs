//! WebSocket types.

use thiserror::Error;

use nyquest_interface::websocket as ws_iface;

/// Information about a WebSocket connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebSocketInfo {
    /// The sub-protocol negotiated for the connection, if any.
    pub protocol: Option<String>,
}

impl WebSocketInfo {
    pub(crate) fn from_interface(value: ws_iface::WebSocketInfo) -> Self {
        value.into()
    }
}

impl From<ws_iface::WebSocketInfo> for WebSocketInfo {
    fn from(value: ws_iface::WebSocketInfo) -> Self {
        Self {
            protocol: value.protocol,
        }
    }
}

impl From<WebSocketInfo> for ws_iface::WebSocketInfo {
    fn from(value: WebSocketInfo) -> Self {
        Self {
            protocol: value.protocol,
        }
    }
}

/// Errors produced by WebSocket operations.
#[non_exhaustive]
#[derive(Debug, Error)]
pub enum WebSocketError {
    /// The WebSocket URL or protocol is invalid.
    #[error("Invalid WebSocket URL or protocol")]
    InvalidUrl,
    /// An underlying I/O error occurred.
    #[error("IO error")]
    Io(#[from] std::io::Error),
    /// The received message exceeded the configured maximum size.
    #[error("WebSocket message exceeds max response buffer size")]
    MessageTooLarge,
    /// The WebSocket protocol was violated.
    #[error("WebSocket protocol error: {0}")]
    Protocol(String),
}

/// The result of a WebSocket operation.
pub type WebSocketResult<T> = std::result::Result<T, WebSocketError>;

impl WebSocketError {
    pub(crate) fn from_interface(value: ws_iface::WebSocketError) -> Self {
        match value {
            ws_iface::WebSocketError::InvalidUrl => Self::InvalidUrl,
            ws_iface::WebSocketError::Io(e) => Self::Io(e),
            ws_iface::WebSocketError::MessageTooLarge => Self::MessageTooLarge,
            ws_iface::WebSocketError::Protocol(s) => Self::Protocol(s),
        }
    }

    pub(crate) fn into_interface(self) -> ws_iface::WebSocketError {
        match self {
            Self::InvalidUrl => ws_iface::WebSocketError::InvalidUrl,
            Self::Io(e) => ws_iface::WebSocketError::Io(e),
            Self::MessageTooLarge => ws_iface::WebSocketError::MessageTooLarge,
            Self::Protocol(s) => ws_iface::WebSocketError::Protocol(s),
        }
    }
}

impl From<ws_iface::WebSocketError> for WebSocketError {
    fn from(value: ws_iface::WebSocketError) -> Self {
        Self::from_interface(value)
    }
}

impl From<WebSocketError> for ws_iface::WebSocketError {
    fn from(value: WebSocketError) -> Self {
        value.into_interface()
    }
}

/// A WebSocket message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WebSocketMessage {
    /// A UTF-8 text message.
    Utf8(String),
    /// A binary message.
    Bytes(Vec<u8>),
    /// A close frame.
    Close {
        /// The close status code.
        code: u16,
        /// The close reason.
        reason: String,
    },
}

impl WebSocketMessage {
    /// Creates a new UTF-8 text message.
    pub fn text<S: Into<String>>(s: S) -> Self {
        Self::Utf8(s.into())
    }

    /// Creates a new binary message.
    pub fn binary<B: Into<Vec<u8>>>(b: B) -> Self {
        Self::Bytes(b.into())
    }

    /// Returns `true` if the message is a text message.
    pub fn is_text(&self) -> bool {
        matches!(self, Self::Utf8(_))
    }

    /// Returns `true` if the message is a binary message.
    pub fn is_binary(&self) -> bool {
        matches!(self, Self::Bytes(_))
    }

    /// Returns `true` if the message is a close message.
    pub fn is_close(&self) -> bool {
        matches!(self, Self::Close { .. })
    }

    /// Consumes the message and returns its payload as binary data.
    ///
    /// For text messages this returns the UTF-8 bytes, for binary messages the
    /// bytes themselves, and for close messages the close reason bytes.
    pub fn into_data(self) -> Vec<u8> {
        match self {
            Self::Utf8(s) => s.into_bytes(),
            Self::Bytes(b) => b,
            Self::Close { reason, .. } => reason.into_bytes(),
        }
    }

    /// Consumes the message and converts it into a string.
    ///
    /// For text and close messages this returns the text/reason directly. For
    /// binary messages the bytes must be valid UTF-8.
    pub fn into_text(self) -> Result<String, std::str::Utf8Error> {
        match self {
            Self::Utf8(s) => Ok(s),
            Self::Bytes(b) => String::from_utf8(b).map_err(|e| e.utf8_error()),
            Self::Close { reason, .. } => Ok(reason),
        }
    }

    /// Borrows the message payload as text.
    ///
    /// For text and close messages this returns the text/reason directly. For
    /// binary messages the bytes must be valid UTF-8.
    pub fn to_text(&self) -> Result<&str, std::str::Utf8Error> {
        match self {
            Self::Utf8(s) => Ok(s),
            Self::Bytes(b) => std::str::from_utf8(b),
            Self::Close { reason, .. } => Ok(reason),
        }
    }

    pub(crate) fn from_interface(value: ws_iface::WebSocketMessage) -> Self {
        match value {
            ws_iface::WebSocketMessage::Utf8(s) => Self::Utf8(s),
            ws_iface::WebSocketMessage::Bytes(b) => Self::Bytes(b),
            ws_iface::WebSocketMessage::Close { code, reason } => Self::Close { code, reason },
        }
    }

    pub(crate) fn into_interface(self) -> ws_iface::WebSocketMessage {
        match self {
            Self::Utf8(s) => ws_iface::WebSocketMessage::Utf8(s),
            Self::Bytes(b) => ws_iface::WebSocketMessage::Bytes(b),
            Self::Close { code, reason } => ws_iface::WebSocketMessage::Close { code, reason },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_constructor_accepts_str_and_string() {
        assert_eq!(
            WebSocketMessage::text("hi"),
            WebSocketMessage::Utf8("hi".into())
        );
        assert_eq!(
            WebSocketMessage::text(String::from("hi")),
            WebSocketMessage::Utf8("hi".into())
        );
        // Cow<'_, str> also implements Into<String>.
        assert_eq!(
            WebSocketMessage::text(std::borrow::Cow::Borrowed("hi")),
            WebSocketMessage::Utf8("hi".into())
        );
    }

    #[test]
    fn binary_constructor_accepts_vec_and_slice() {
        assert_eq!(
            WebSocketMessage::binary(vec![1u8, 2, 3]),
            WebSocketMessage::Bytes(vec![1, 2, 3])
        );
        assert_eq!(
            WebSocketMessage::binary([1u8, 2, 3]),
            WebSocketMessage::Bytes(vec![1, 2, 3])
        );
        let slice: &[u8] = &[1, 2, 3];
        assert_eq!(
            WebSocketMessage::binary(slice),
            WebSocketMessage::Bytes(vec![1, 2, 3])
        );
    }

    #[test]
    fn is_flags_cover_all_variants() {
        let text = WebSocketMessage::Utf8("hi".into());
        assert!(text.is_text());
        assert!(!text.is_binary());
        assert!(!text.is_close());

        let binary = WebSocketMessage::Bytes(vec![1]);
        assert!(!binary.is_text());
        assert!(binary.is_binary());
        assert!(!binary.is_close());

        let close = WebSocketMessage::Close {
            code: 1000,
            reason: "done".into(),
        };
        assert!(!close.is_text());
        assert!(!close.is_binary());
        assert!(close.is_close());
    }

    #[test]
    fn into_data_covers_all_variants() {
        assert_eq!(
            WebSocketMessage::Utf8("hi".into()).into_data(),
            b"hi".to_vec()
        );
        assert_eq!(
            WebSocketMessage::Bytes(vec![1, 2, 3]).into_data(),
            vec![1, 2, 3]
        );
        assert_eq!(
            WebSocketMessage::Close {
                code: 1000,
                reason: "bye".into(),
            }
            .into_data(),
            b"bye".to_vec()
        );
        // Empty reason yields empty payload, mirroring tungstenite.
        assert!(WebSocketMessage::Close {
            code: 1000,
            reason: String::new(),
        }
        .into_data()
        .is_empty());
    }

    #[test]
    fn into_text_covers_all_variants() {
        // Text passes through.
        assert_eq!(
            WebSocketMessage::Utf8("hi".into()).into_text().unwrap(),
            "hi"
        );
        // Valid UTF-8 binary converts.
        assert_eq!(
            WebSocketMessage::Bytes(b"hi".to_vec()).into_text().unwrap(),
            "hi"
        );
        // Invalid UTF-8 binary errors with Utf8Error.
        let err = WebSocketMessage::Bytes(vec![0xff]).into_text().unwrap_err();
        let _: std::str::Utf8Error = err;
        // Close yields its reason.
        assert_eq!(
            WebSocketMessage::Close {
                code: 1000,
                reason: "done".into(),
            }
            .into_text()
            .unwrap(),
            "done"
        );
    }

    #[test]
    fn to_text_covers_all_variants() {
        assert_eq!(WebSocketMessage::Utf8("hi".into()).to_text().unwrap(), "hi");
        assert_eq!(
            WebSocketMessage::Bytes(b"hi".to_vec()).to_text().unwrap(),
            "hi"
        );
        assert!(WebSocketMessage::Bytes(vec![0xff])
            .to_text()
            .is_err_and(|e| {
                let _: std::str::Utf8Error = e;
                true
            }));
        assert_eq!(
            WebSocketMessage::Close {
                code: 1001,
                reason: "bye".into(),
            }
            .to_text()
            .unwrap(),
            "bye"
        );
    }

    #[test]
    fn interface_conversions_cover_all_variants() {
        for iface in [
            ws_iface::WebSocketMessage::Utf8("hi".into()),
            ws_iface::WebSocketMessage::Bytes(vec![1, 2]),
            ws_iface::WebSocketMessage::Close {
                code: 1000,
                reason: "done".into(),
            },
        ] {
            let owned = WebSocketMessage::from_interface(iface.clone());
            assert_eq!(owned.clone().into_interface(), iface);
        }

        for owned in [
            WebSocketMessage::Utf8("hi".into()),
            WebSocketMessage::Bytes(vec![1, 2]),
            WebSocketMessage::Close {
                code: 1000,
                reason: "done".into(),
            },
        ] {
            let iface = owned.clone().into_interface();
            assert_eq!(WebSocketMessage::from_interface(iface), owned);
        }
    }
}
