//! Stream upload support for WinHTTP backend.
//!
//! This module provides support for streaming request bodies, including
//! multipart uploads with stream parts.

#[cfg(any(feature = "async-stream", feature = "blocking-stream"))]
mod writer;
#[cfg(not(any(feature = "async-stream", feature = "blocking-stream")))]
#[path = "stream/dummy_writer.rs"]
mod writer;

pub(crate) use writer::StreamWriter;

/// Represents either data bytes or a stream.
pub enum DataOrStream<S> {
    /// A chunk of bytes to be sent.
    #[allow(dead_code)]
    Data(Vec<u8>),
    /// A stream to read from.
    Stream(S),
}
