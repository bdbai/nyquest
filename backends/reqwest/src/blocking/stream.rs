use std::io::{self, Read as _};
use std::pin::Pin;
use std::sync::Mutex;
use std::task::{Context, Poll};

use bytes::BytesMut;
use futures::FutureExt;
use nyquest_interface::blocking::BoxedStream;
use tokio::task::JoinHandle;

const READ_BUFFER_SIZE: usize = 16 * 1024;

enum MaybeReadingStream {
    Invalid,
    Ready(BoxedStream),
    Reading(JoinHandle<io::Result<(BytesMut, BoxedStream)>>),
}

pub(super) struct BlockingStreamBody {
    stream_maybe_reading: Mutex<MaybeReadingStream>,
    remaining_size: Option<u64>,
}

impl BlockingStreamBody {
    pub fn new(stream: BoxedStream) -> Self {
        let remaining_size = match &stream {
            BoxedStream::Sized { content_length, .. } => Some(*content_length),
            BoxedStream::Unsized { .. } => None,
        };
        Self {
            stream_maybe_reading: Mutex::new(MaybeReadingStream::Ready(stream)),
            remaining_size,
        }
    }
}

// Perform std::io::Read::read() in a spawn_blocking context
impl http_body::Body for BlockingStreamBody {
    type Data = BytesMut;

    type Error = io::Error;

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<http_body::Frame<Self::Data>, Self::Error>>> {
        let this = self.get_mut();
        let stream_maybe_reading = this.stream_maybe_reading.get_mut().unwrap();
        loop {
            match std::mem::replace(stream_maybe_reading, MaybeReadingStream::Invalid) {
                MaybeReadingStream::Invalid => panic!("polled after completion"),
                MaybeReadingStream::Ready(mut stream) => {
                    let read_fut = tokio::task::spawn_blocking(move || {
                        let mut buf = BytesMut::zeroed(READ_BUFFER_SIZE);
                        let n = stream.read(&mut buf)?;
                        buf.truncate(n);
                        Ok((buf, stream))
                    });
                    *stream_maybe_reading = MaybeReadingStream::Reading(read_fut);
                }
                MaybeReadingStream::Reading(mut handle) => match handle.poll_unpin(cx) {
                    Poll::Pending => {
                        *stream_maybe_reading = MaybeReadingStream::Reading(handle);
                        break Poll::Pending;
                    }
                    Poll::Ready(Err(e)) => panic!("blocking task panicked: {e}"),
                    Poll::Ready(Ok(Ok((buf, stream)))) => {
                        *stream_maybe_reading = MaybeReadingStream::Ready(stream);
                        if buf.is_empty() {
                            break Poll::Ready(None);
                        } else {
                            if let Some(remaining_size) = &mut this.remaining_size {
                                *remaining_size = remaining_size.saturating_sub(buf.len() as u64);
                            }
                            break Poll::Ready(Some(Ok(http_body::Frame::data(buf))));
                        }
                    }
                    Poll::Ready(Ok(Err(e))) => break Poll::Ready(Some(Err(e))),
                },
            }
        }
    }

    fn is_end_stream(&self) -> bool {
        self.remaining_size == Some(0)
    }

    fn size_hint(&self) -> http_body::SizeHint {
        self.remaining_size
            .map(http_body::SizeHint::with_exact)
            .unwrap_or_default()
    }
}
