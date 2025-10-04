use std::{
    future::{pending, poll_fn},
    io,
    pin::Pin,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    task::{ready, Context, Poll},
};

use bytes::BytesMut;
use futures::{future::join_all, task::AtomicWaker, AsyncReadExt};
use nyquest_interface::r#async::BoxedStream;
use tokio::sync::mpsc;

const READ_BUFFER_SIZE: usize = 16 * 1024;

#[derive(Default)]
struct ChannelOpenState {
    waker: AtomicWaker,
    is_open: AtomicBool,
}

struct StreamTask {
    stream: BoxedStream,
    open_state: Arc<ChannelOpenState>,
    chan: mpsc::Sender<io::Result<bytes::BytesMut>>,
}

#[derive(Default)]
pub(super) struct StreamTaskCollection {
    tasks: Vec<StreamTask>,
}

pub(super) struct AsyncStreamBody {
    open_state: Option<Arc<ChannelOpenState>>,
    chan: mpsc::Receiver<io::Result<bytes::BytesMut>>,
    remaining_size: Option<u64>,
}

impl ChannelOpenState {
    fn set_open(&self) {
        self.is_open.store(true, Ordering::SeqCst);
        self.waker.wake();
    }

    async fn wait_until_open(&self) {
        poll_fn(|cx| {
            self.waker.register(cx.waker());
            if self.is_open.load(Ordering::SeqCst) {
                Poll::Ready(())
            } else {
                Poll::Pending
            }
        })
        .await;
    }
}

impl StreamTask {
    async fn run(mut self) {
        self.open_state.wait_until_open().await;
        let mut chan_res = Ok(());
        while chan_res.is_ok() {
            let mut buf = BytesMut::zeroed(READ_BUFFER_SIZE);
            let res = self.stream.read(&mut buf).await;
            let read_len = match res {
                Ok(len) => len,
                Err(e) => {
                    self.chan.send(Err(e)).await.ok();
                    break;
                }
            };
            buf.truncate(read_len);
            chan_res = self.chan.send(Ok(buf)).await;
            if read_len == 0 {
                break;
            }
        }
    }
}

impl StreamTaskCollection {
    pub fn add_stream(&mut self, stream: BoxedStream) -> AsyncStreamBody {
        let size = stream.content_length();

        let (chan, recv) = mpsc::channel(1);
        let open_state = Arc::new(ChannelOpenState::default());
        self.tasks.push(StreamTask {
            stream,
            open_state: open_state.clone(),
            chan,
        });
        AsyncStreamBody {
            open_state: Some(open_state),
            chan: recv,
            remaining_size: size,
        }
    }

    pub async fn execute(self) {
        join_all(self.tasks.into_iter().map(StreamTask::run)).await;
        pending().await
    }
}

impl http_body::Body for AsyncStreamBody {
    type Data = bytes::Bytes;

    type Error = io::Error;

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<http_body::Frame<Self::Data>, Self::Error>>> {
        let this = self.get_mut();
        if let Some(open_state) = this.open_state.take() {
            open_state.set_open();
        }
        let res = ready!(this.chan.poll_recv(cx)?);
        Poll::Ready(if let Some(res) = res.filter(|res| !res.is_empty()) {
            if let Some(remaining_size) = &mut this.remaining_size {
                *remaining_size = remaining_size.saturating_sub(res.len() as u64);
            }
            Some(Ok(http_body::Frame::data(res.into())))
        } else {
            None
        })
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
