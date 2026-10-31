//! Common WebSocket data and error types.

use thiserror::Error;

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

/// Errors produced by WebSocket operations.
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
