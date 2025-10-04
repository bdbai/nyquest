use std::{
    future::{poll_fn, Future as _},
    io::{self, SeekFrom},
    ops::ControlFlow,
    pin::{pin, Pin},
    sync::Arc,
    task::{ready, Context, Poll},
};

use curl::easy::{ReadError, SeekResult};
use futures_util::AsyncRead as _;
use nyquest_interface::r#async::BoxedStream;

use crate::r#async::AsyncHandler;
use crate::r#async::{r#loop::SendUnpauser, shared::SharedRequestStates};
use crate::request::EasyHandle;
use crate::{
    curl_ng::{
        easy::AsRawEasyMut as _,
        mime::{MimePartContent, MimePartReader},
        CurlCodeContext,
    },
    r#async::r#loop::LoopManager,
};

pub(super) struct SharedStreamState {
    buf: Vec<u8>,
    request: SharedStreamRequest,
    can_seek: bool,
}

#[derive(Debug, Default, Clone)]
enum SharedStreamRequest {
    #[default]
    Ready,
    Read(usize),
    Seek(SeekFrom),
    Fail,
    Eof,
}

pub(super) struct AsyncStreamReader {
    ctx: Arc<SharedRequestStates>,
    id: usize,
}

pub(super) struct ReadTaskCollection {
    shared: Arc<SharedRequestStates>,
    streams: Vec<BoxedStream>,
}

impl MimePartReader for AsyncStreamReader {
    fn read(&mut self, data: &mut [u8]) -> Result<usize, ReadError> {
        let mut state = self.ctx.state.lock().unwrap();
        let Some(s) = state.req_streams.get_mut(self.id) else {
            return Err(ReadError::Abort);
        };
        s.read(data, &self.ctx)
    }

    fn seek(&mut self, whence: io::SeekFrom) -> SeekResult {
        let mut state = self.ctx.state.lock().unwrap();
        let Some(s) = state.req_streams.get_mut(self.id) else {
            return SeekResult::Fail;
        };
        s.seek(whence, &self.ctx)
    }
}

impl ReadTaskCollection {
    pub fn new(shared: Arc<SharedRequestStates>) -> Self {
        Self {
            shared,
            streams: vec![],
        }
    }

    pub fn add_in_handler(
        &mut self,
        mut easy: Pin<&mut EasyHandle<AsyncHandler>>,
        stream: BoxedStream,
    ) -> Result<(), CurlCodeContext> {
        if let BoxedStream::Sized { content_length, .. } = &stream {
            easy.as_mut()
                .as_raw_easy_mut()
                .set_post_field_size(*content_length)?;
            add_stream(&self.shared, true);
            Some(*content_length as i64)
        } else {
            add_stream(&self.shared, false);
            None
        };
        self.streams.push(stream);
        Ok(())
    }

    pub fn add_mime_part_reader(
        &mut self,
        stream: BoxedStream,
    ) -> MimePartContent<AsyncStreamReader> {
        let size = if let BoxedStream::Sized { content_length, .. } = &stream {
            add_stream(&self.shared, true);
            Some(*content_length as i64)
        } else {
            add_stream(&self.shared, false);
            None
        };
        let id = self.streams.len();
        self.streams.push(stream);
        MimePartContent::Reader {
            reader: AsyncStreamReader {
                id,
                ctx: self.shared.clone(),
            },
            size,
        }
    }

    pub async fn execute(mut self, r#loop: &LoopManager) -> nyquest_interface::Result<()> {
        let shared = self.shared.clone();
        let mut fut = pin!(r#loop.batch_unpause_send(&shared, |cx, unpauser| {
            let mut state = shared.state.lock().unwrap();
            let iter = state.req_streams.iter_mut().zip(self.streams.iter_mut());
            for (shared, stream) in iter {
                if let Err(e) = poll_execute_one(cx, shared, stream, unpauser) {
                    return ControlFlow::Break(e);
                }
            }
            ControlFlow::Continue(())
        }));
        poll_fn(|cx| {
            let res = ready!(fut.as_mut().poll(cx));
            let cb = match res {
                ControlFlow::Continue(cb) => cb,
                ControlFlow::Break(e) => return Poll::Ready(Err(e)),
            };
            fut.set(r#loop.batch_unpause_send(&shared, cb));
            Poll::Pending
        })
        .await
    }
}

fn poll_execute_one(
    cx: &mut Context<'_>,
    shared: &mut SharedStreamState,
    stream: &mut BoxedStream,
    unpauser: &mut SendUnpauser<'_>,
) -> nyquest_interface::Result<()> {
    match shared.request {
        SharedStreamRequest::Read(size_hint) => {
            shared.buf.resize(size_hint, 0);
            match Pin::new(stream).poll_read(cx, &mut shared.buf) {
                Poll::Pending => {}
                Poll::Ready(Ok(len)) => {
                    shared.buf.truncate(len);
                    shared.request = if len == 0 {
                        SharedStreamRequest::Eof
                    } else {
                        SharedStreamRequest::Ready
                    };
                    unpauser.unpause_send();
                }
                Poll::Ready(Err(e)) => {
                    shared.request = SharedStreamRequest::Fail;
                    unpauser.unpause_send();
                    return Err(nyquest_interface::Error::Io(e));
                }
            }
        }
        SharedStreamRequest::Seek(pos) => {
            let stream = match stream {
                BoxedStream::Sized { stream, .. } => stream,
                BoxedStream::Unsized { .. } => {
                    shared.request = SharedStreamRequest::Fail;
                    unpauser.unpause_send();
                    return Err(nyquest_interface::Error::Io(io::Error::other(
                        "Cannot seek on unsized stream",
                    )));
                }
            };
            match stream.as_mut().poll_seek(cx, pos) {
                Poll::Pending => {}
                Poll::Ready(Err(e)) => {
                    shared.request = SharedStreamRequest::Fail;
                    unpauser.unpause_send();
                    return Err(nyquest_interface::Error::Io(e));
                }
                Poll::Ready(Ok(_)) => {
                    unpauser.unpause_send();
                    shared.request = SharedStreamRequest::Ready;
                }
            }
        }
        SharedStreamRequest::Ready | SharedStreamRequest::Eof => {}
        SharedStreamRequest::Fail => unreachable!(),
    }
    Ok(())
}

fn add_stream(shared: &SharedRequestStates, can_seek: bool) {
    let mut state = shared.state.lock().unwrap();
    state.req_streams.push(SharedStreamState {
        buf: vec![],
        request: SharedStreamRequest::Ready,
        can_seek,
    });
}

impl SharedStreamState {
    pub(super) fn read(
        &mut self,
        data: &mut [u8],
        shared: &SharedRequestStates,
    ) -> Result<usize, ReadError> {
        let read_len = data.len().min(self.buf.len());
        match &self.request {
            SharedStreamRequest::Fail => return Err(ReadError::Abort),
            SharedStreamRequest::Read(_) | SharedStreamRequest::Seek(_) => {
                return Err(ReadError::Pause)
            }
            SharedStreamRequest::Ready if read_len == 0 => {
                self.request = SharedStreamRequest::Read(data.len());
                shared.waker.wake();
                return Err(ReadError::Pause);
            }
            SharedStreamRequest::Eof if read_len == 0 => return Ok(0),
            SharedStreamRequest::Ready | SharedStreamRequest::Eof => {}
        };
        data[..read_len].copy_from_slice(&self.buf[..read_len]);
        self.buf.drain(..read_len);

        Ok(read_len)
    }

    pub(super) fn seek(
        &mut self,
        whence: io::SeekFrom,
        shared: &SharedRequestStates,
    ) -> SeekResult {
        if !self.can_seek {
            return SeekResult::CantSeek;
        }
        if let SharedStreamRequest::Fail = &self.request {
            return SeekResult::Fail;
        }
        self.request = SharedStreamRequest::Seek(whence);
        shared.waker.wake();
        SeekResult::Ok
    }
}
