use std::{io, ops::ControlFlow, task::Poll};

use objc2::rc::{Retained, Weak as WeakRetained};

use crate::stream::{DataOrStream, InputStream};

pub(crate) struct StreamWriter<S> {
    stream: WeakRetained<InputStream>,
    data_parts: Vec<DataOrStream<S>>,
}

impl<S> StreamWriter<S> {
    pub fn new(stream: &Retained<InputStream>, data_parts: Vec<DataOrStream<S>>) -> Self {
        Self {
            stream: WeakRetained::from_retained(stream),
            data_parts,
        }
    }

    #[cfg(any(feature = "async-stream", feature = "blocking-stream"))]
    pub fn poll_progress(
        &mut self,
        mut read_cb: impl FnMut(&mut S, &mut [u8]) -> Poll<io::Result<usize>>,
    ) -> io::Result<bool> {
        let Some(stream) = self.stream.load() else {
            return Ok(false);
        };
        if !stream.is_open() {
            return Ok(true);
        }
        stream.update_buffer(|mut buf| {
            let mut total_written = 0;
            loop {
                if buf.is_empty() {
                    break;
                }
                let Some(part) = self.data_parts.first_mut() else {
                    break;
                };
                if let DataOrStream::Data(data) = part {
                    if data.is_empty() {
                        self.data_parts.remove(0);
                        continue;
                    }
                }
                let write_result = match part {
                    DataOrStream::Data(data) => {
                        let to_write = data.len().min(buf.len());
                        buf[..to_write].copy_from_slice(data.drain(..to_write).as_slice());
                        Poll::Ready(Ok(to_write))
                    }
                    DataOrStream::Stream(s) => read_cb(s, buf),
                };
                match write_result {
                    Poll::Ready(Ok(0)) => {
                        self.data_parts.remove(0);
                    }
                    Poll::Ready(Ok(n)) => {
                        total_written += n;
                        buf = &mut buf[n..];
                    }
                    Poll::Ready(Err(e)) => return ControlFlow::Continue(Err(e)),
                    Poll::Pending if total_written == 0 => return ControlFlow::Break(()),
                    Poll::Pending => break,
                }
            }
            ControlFlow::Continue(Ok(total_written))
        })?;
        Ok(!self.data_parts.is_empty())
    }
}
