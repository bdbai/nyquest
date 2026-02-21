use std::{
    io::{self},
    ops::Range,
    task::Poll,
};

use objc2::rc::{Retained, Weak as WeakRetained};

use crate::stream::{DataOrStream, InputStream};

pub(crate) struct StreamWriter<S> {
    stream: WeakRetained<InputStream>,
    data_parts: Vec<DataOrStream<S>>,
    buffer: Vec<u8>,
    // When the next part is a data part, this is the range of the data part that has not yet been written.
    // When the next part is a stream part,
    // - if the buffer has not yet been filled, this is 0..0,
    // - if the buffer has been filled, this is the range of the buffer that has not yet been written.
    buffer_range: Range<usize>,
}

impl<S> StreamWriter<S> {
    pub fn new(stream: &Retained<InputStream>, data_parts: Vec<DataOrStream<S>>) -> Self {
        Self {
            stream: WeakRetained::from_retained(stream),
            buffer: if data_parts
                .iter()
                .any(|p| matches!(p, DataOrStream::Stream(_)))
            {
                vec![0; crate::stream::STREAM_BUFFER_SIZE]
            } else {
                vec![]
            },
            buffer_range: get_range_of_part(data_parts.first()),
            data_parts,
        }
    }

    fn shift_part(&mut self) {
        self.data_parts.remove(0);
        self.buffer_range = get_range_of_part(self.data_parts.first());
    }

    #[cfg(any(feature = "async-stream", feature = "blocking-stream"))]
    fn poll_progress_once(
        &mut self,
        mut read_cb: impl FnMut(&mut S, &mut [u8]) -> Poll<io::Result<usize>>,
        stream: &InputStream,
    ) -> Poll<io::Result<usize>> {
        use objc2_foundation::{NSError, NSString};

        let buf = loop {
            let Some(part) = self.data_parts.first_mut() else {
                break Ok(&[][..]);
            };

            match part {
                DataOrStream::Data(_) if self.buffer_range.is_empty() => {
                    self.shift_part();
                    continue;
                }
                DataOrStream::Data(data) => {
                    break Ok(&data[self.buffer_range.clone()]);
                }
                DataOrStream::Stream(stream) => {
                    use std::task::ready;

                    if !self.buffer_range.is_empty() {
                        break Ok(&self.buffer[self.buffer_range.clone()]);
                    }
                    match ready!(read_cb(stream, &mut self.buffer[..])) {
                        Err(e) => break Err(e),
                        Ok(0) => {
                            self.shift_part();
                            continue;
                        }
                        Ok(read) => {
                            self.buffer_range = 0..read;
                            break Ok(&self.buffer[self.buffer_range.clone()]);
                        }
                    }
                }
            }
        };
        let written = stream.write(buf.as_deref().map_err(|e| {
            NSError::new(
                e.raw_os_error().unwrap_or_default() as _,
                &NSString::from_str(&e.to_string()),
            )
        }));
        buf?;
        self.buffer_range = self.buffer_range.start + written..self.buffer_range.end;
        Poll::Ready(Ok(written))
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

        loop {
            match self.poll_progress_once(&mut read_cb, &stream) {
                Poll::Ready(Ok(0)) => break Ok(!self.data_parts.is_empty()),
                Poll::Ready(Ok(_)) => continue,
                Poll::Ready(Err(e)) => break Err(e),
                Poll::Pending => break Ok(true),
            }
        }
    }
}

fn get_range_of_part<S>(part: Option<&DataOrStream<S>>) -> Range<usize> {
    match part {
        Some(DataOrStream::Data(data)) => 0..data.len(),
        Some(DataOrStream::Stream(_)) => 0..0,
        None => 0..0,
    }
}
