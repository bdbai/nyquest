//! Stream writer for WinHTTP backend.
//!
//! This follows the pattern from nsurlsession backend.

use std::io::{self, Cursor};
use std::ops::Range;
use std::task::{ready, Poll};

use super::DataOrStream;

/// Buffer size for chunked transfers.
const CHUNK_BUFFER_SIZE: usize = 16 * 1024;

/// Writes stream data to a WinHTTP request.
pub(crate) struct StreamWriter<S> {
    /// Parts to write (data chunks or streams).
    data_parts: Vec<DataOrStream<S>>,
    /// Internal buffer for reading from streams.
    buffer: Vec<u8>,
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
            chunked,
            finished: false,
        }
    }

    /// Returns true if writing is complete.
    pub fn is_finished(&self) -> bool {
        self.finished
    }

    fn get_max_chunk_size(&self) -> usize {
        if self.chunked {
            CHUNK_BUFFER_SIZE - 12
        } else {
            CHUNK_BUFFER_SIZE
        }
    }

    pub fn poll_take_buffer(
        &mut self,
        mut read_cb: impl FnMut(&mut S, &mut [u8]) -> Poll<io::Result<usize>>,
    ) -> Poll<io::Result<(Vec<u8>, Range<usize>)>> {
        if self.finished {
            return Poll::Ready(Ok((Vec::new(), 0..0)));
        }

        let max_chunk_size = self.get_max_chunk_size();
        let (mut range, mut buf) = loop {
            match self.data_parts.first_mut() {
                Some(DataOrStream::Data(data)) if data.is_empty() => {
                    self.data_parts.remove(0);
                    continue;
                }
                Some(DataOrStream::Data(data)) => break (0..data.len(), std::mem::take(data)),
                Some(DataOrStream::Stream(stream)) => {
                    self.buffer.resize(CHUNK_BUFFER_SIZE, 0);
                    let res = ready!(read_cb(stream, &mut self.buffer[..max_chunk_size]))?;
                    if res == 0 {
                        self.data_parts.remove(0);
                        continue;
                    }
                    break (0..res, std::mem::take(&mut self.buffer));
                }
                _ => {
                    // No more parts, we're done
                    self.finished = true;
                    break (0..0, vec![]);
                }
            }
        };

        if self.chunked {
            if range.is_empty() {
                self.buffer.resize(5, 0);
                self.buffer[..5].copy_from_slice(b"0\r\n\r\n");
                (buf, range) = (std::mem::take(&mut self.buffer), 0..5);
            } else {
                decorate_buffer_for_chunked_encoding(&mut buf, &mut range);
            }
        }
        Poll::Ready(Ok((buf, range)))
    }

    #[cfg(feature = "async-stream")]
    pub async fn take_buffer(
        &mut self,
        mut read_cb: impl FnMut(
            &mut S,
            &mut [u8],
            &mut std::task::Context<'_>,
        ) -> Poll<io::Result<usize>>,
    ) -> io::Result<(Vec<u8>, Range<usize>)> {
        std::future::poll_fn(|cx| self.poll_take_buffer(|stream, buf| read_cb(stream, buf, cx)))
            .await
    }

    pub fn advance(&mut self, mut buffer_to_return: Vec<u8>) {
        match self.data_parts.first_mut() {
            Some(DataOrStream::Data(_)) => {
                // Drop the buffer
            }
            Some(DataOrStream::Stream(_)) => {
                buffer_to_return.resize(CHUNK_BUFFER_SIZE, 0);
                self.buffer = buffer_to_return;
            }
            None => {}
        }
    }
}

fn decorate_buffer_for_chunked_encoding(buffer: &mut Vec<u8>, range: &mut Range<usize>) {
    use std::io::Write as _;

    let encoding_buf = {
        let mut buf = Cursor::new([0u8; 12]);
        write!(buf, "{:X}\r\n\r\n", range.len())
            .expect("chunked encoding buf should not exceed 12 bytes");
        buf
    };
    let encoding_buf = &encoding_buf.get_ref()[..encoding_buf.position() as usize];
    if buffer.len() < range.end + encoding_buf.len() {
        buffer.truncate(range.end);
        buffer.extend_from_slice(encoding_buf);
    } else {
        buffer[range.end..][..encoding_buf.len()].copy_from_slice(encoding_buf);
    }
    let encoding_front_len = encoding_buf.len() - 2;
    buffer[range.start..range.end + encoding_front_len].rotate_right(encoding_front_len);
    range.end += encoding_buf.len();
}
