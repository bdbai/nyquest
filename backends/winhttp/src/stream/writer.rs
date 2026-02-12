//! Stream writer for WinHTTP backend.
//!
//! This follows the pattern from nsurlsession backend.

use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

use super::DataOrStream;

/// Buffer size for chunked transfers.
const CHUNK_BUFFER_SIZE: usize = 8192;

/// Writes stream data to a WinHTTP request.
pub(crate) struct StreamWriter<S> {
    /// Parts to write (data chunks or streams).
    data_parts: Vec<DataOrStream<S>>,
    /// Internal buffer for reading from streams.
    buffer: Vec<u8>,
    /// Number of valid bytes in the buffer.
    buffer_len: usize,
    /// Whether we're using chunked transfer encoding.
    chunked: bool,
    /// Whether we've finished writing all data.
    finished: bool,
}

impl<S> StreamWriter<S> {
    /// Creates a new stream writer.
    pub fn new(data_parts: Vec<DataOrStream<S>>, chunked: bool) -> Self {
        Self {
            data_parts,
            buffer: vec![0u8; CHUNK_BUFFER_SIZE],
            buffer_len: 0,
            chunked,
            finished: false,
        }
    }

    /// Returns true if writing is complete.
    pub fn is_finished(&self) -> bool {
        self.finished
    }

    /// Tries to fill the internal buffer by reading from parts using a callback.
    ///
    /// Returns:
    /// - `Poll::Ready(Ok(true))` if data was produced (check `take_pending_data`)
    /// - `Poll::Ready(Ok(false))` if all data has been written (call `get_final_chunk` if chunked)
    /// - `Poll::Ready(Err(e))` if an error occurred
    /// - `Poll::Pending` if we need to wait for the stream to produce data
    pub fn poll_fill_buffer_with_cb(
        &mut self,
        mut read_cb: impl FnMut(&mut S, &mut [u8]) -> Poll<io::Result<usize>>,
    ) -> Poll<io::Result<bool>> {
        if self.finished {
            return Poll::Ready(Ok(false));
        }

        loop {
            // If we have pending data in buffer, don't read more until it's consumed
            if self.buffer_len > 0 {
                return Poll::Ready(Ok(true));
            }

            let Some(part) = self.data_parts.first_mut() else {
                // No more parts, we're done
                self.finished = true;
                return Poll::Ready(Ok(false));
            };

            match part {
                DataOrStream::Data(data) => {
                    if data.is_empty() {
                        self.data_parts.remove(0);
                        continue;
                    }
                    // Copy data to buffer
                    let to_write = data.len().min(self.buffer.len());
                    self.buffer[..to_write].copy_from_slice(&data[..to_write]);
                    self.buffer_len = to_write;
                    data.drain(..to_write);
                    return Poll::Ready(Ok(true));
                }
                DataOrStream::Stream(stream) => {
                    match read_cb(stream, &mut self.buffer) {
                        Poll::Ready(Ok(0)) => {
                            // Stream exhausted
                            self.data_parts.remove(0);
                        }
                        Poll::Ready(Ok(n)) => {
                            self.buffer_len = n;
                            return Poll::Ready(Ok(true));
                        }
                        Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
                        Poll::Pending => return Poll::Pending,
                    }
                }
            }
        }
    }

    /// Gets the pending data to write, formatted for the transfer encoding.
    ///
    /// Returns the data to write and clears the internal buffer.
    pub fn take_pending_data(&mut self) -> Vec<u8> {
        if self.buffer_len == 0 {
            return Vec::new();
        }

        let data = if self.chunked {
            // Format as chunked: <size in hex>\r\n<data>\r\n
            let mut chunk = format!("{:X}\r\n", self.buffer_len).into_bytes();
            chunk.extend_from_slice(&self.buffer[..self.buffer_len]);
            chunk.extend_from_slice(b"\r\n");
            chunk
        } else {
            self.buffer[..self.buffer_len].to_vec()
        };

        self.buffer_len = 0;
        data
    }

    /// Gets the final chunk for chunked encoding (the terminator).
    pub fn get_final_chunk(&self) -> &'static [u8] {
        b"0\r\n\r\n"
    }
}

// Async stream support
#[cfg(feature = "async-stream")]
impl StreamWriter<nyquest_interface::r#async::BoxedStream> {
    /// Async version of fill_buffer that works with BoxedStream.
    ///
    /// Fills the internal buffer from the next data part or stream.
    /// Returns the number of bytes available to write, or 0 if done.
    pub fn poll_fill_buffer(
        &mut self,
        cx: &mut Context<'_>,
        output_buffer: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        use nyquest_interface::r#async::futures_io::AsyncRead;

        if self.finished {
            return Poll::Ready(Ok(0));
        }

        loop {
            let Some(part) = self.data_parts.first_mut() else {
                // No more parts, we're done
                self.finished = true;
                return Poll::Ready(Ok(0));
            };

            match part {
                DataOrStream::Data(data) => {
                    if data.is_empty() {
                        self.data_parts.remove(0);
                        continue;
                    }
                    // Copy data to output buffer
                    let to_write = data.len().min(output_buffer.len());

                    let output = if self.chunked {
                        // Format as chunked: <size in hex>\r\n<data>\r\n
                        let mut chunk = format!("{:X}\r\n", to_write).into_bytes();
                        chunk.extend_from_slice(&data[..to_write]);
                        chunk.extend_from_slice(b"\r\n");
                        data.drain(..to_write);

                        // Copy to output buffer
                        let len = chunk.len().min(output_buffer.len());
                        output_buffer[..len].copy_from_slice(&chunk[..len]);
                        len
                    } else {
                        output_buffer[..to_write].copy_from_slice(&data[..to_write]);
                        data.drain(..to_write);
                        to_write
                    };

                    return Poll::Ready(Ok(output));
                }
                DataOrStream::Stream(stream) => {
                    // Use a temporary buffer to read from the stream
                    let temp_buf = if self.chunked {
                        // Reserve space for chunk header/trailer
                        &mut self.buffer[..]
                    } else {
                        &mut output_buffer[..]
                    };

                    let pinned = Pin::new(stream);
                    match pinned.poll_read(cx, temp_buf) {
                        Poll::Ready(Ok(0)) => {
                            // Stream exhausted
                            self.data_parts.remove(0);
                        }
                        Poll::Ready(Ok(n)) => {
                            let output = if self.chunked {
                                // Format as chunked: <size in hex>\r\n<data>\r\n
                                let mut chunk = format!("{:X}\r\n", n).into_bytes();
                                chunk.extend_from_slice(&self.buffer[..n]);
                                chunk.extend_from_slice(b"\r\n");

                                // Copy to output buffer
                                let len = chunk.len().min(output_buffer.len());
                                output_buffer[..len].copy_from_slice(&chunk[..len]);
                                len
                            } else {
                                n
                            };
                            return Poll::Ready(Ok(output));
                        }
                        Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
                        Poll::Pending => return Poll::Pending,
                    }
                }
            }
        }
    }
}

/// Sync version of poll_fill_buffer for blocking client.
#[cfg(feature = "blocking-stream")]
impl<S: std::io::Read> StreamWriter<S> {
    /// Blocking version of fill_buffer.
    pub fn fill_buffer_blocking(&mut self) -> io::Result<bool> {
        match self.poll_fill_buffer_with_cb(|stream, buf| Poll::Ready(stream.read(buf))) {
            Poll::Ready(result) => result,
            Poll::Pending => unreachable!("blocking read should not return Pending"),
        }
    }
}
