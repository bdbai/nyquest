use std::collections::VecDeque;
use std::future::poll_fn;
use std::sync::{Arc, Mutex, Weak};
use std::task::{Context, Poll};
use std::{io, thread};

use futures_channel::oneshot;
use futures_util::lock::Mutex as FuturesMutex;
use futures_util::task::AtomicWaker;
use nyquest_interface::Result as NyquestResult;

use crate::curl_ng::easy::AsRawEasyMut as _;
use crate::curl_ng::multi::{MultiWaker, MultiWithSet, RawMulti, WakeableMulti};
use crate::r#async::set::SlabMultiSet;
use crate::r#async::AsyncHandler;
use crate::request::{AsCallbackMut as _, BoxEasyHandle};
use crate::state::RequestState;

type Easy = BoxEasyHandle<super::handler::AsyncHandler>;

pub(super) struct RequestHandle {
    id: usize,
    shared_context: Arc<SharedRequestContext>,
    manager: LoopManagerShared,
}

#[derive(Default)]
enum SharedRequestContextResult {
    #[default]
    Init,
    InProgress {
        id: usize,
    },
    EasyLost(Easy),
    Done {
        res: NyquestResult<()>,
        id: usize,
    },
}

#[derive(Default)]
pub(super) struct SharedRequestContextState {
    pub(super) state: RequestState,
    result: SharedRequestContextResult,
    response: Option<super::CurlAsyncResponse>,
}

#[derive(Default)]
pub(super) struct SharedRequestContext {
    pub(super) waker: AtomicWaker,
    pub(super) state: Mutex<SharedRequestContextState>,
}

enum LoopTask {
    ConstructHandle(Easy),
    QueryHandleResponse(RequestHandle),
    UnpauseRecvHandle(usize),
    _UnpauseSendHandle(usize),
    DropHandle(usize),
    Shutdown,
}

impl RequestHandle {
    pub(super) async fn wait_for_response(
        self,
    ) -> nyquest_interface::Result<super::CurlAsyncResponse> {
        poll_fn(|cx| {
            let mut state = self.shared_context.state.lock().unwrap();
            match std::mem::take(&mut state.result) {
                SharedRequestContextResult::Done { res: Ok(()), id } => {
                    // Do not take out result if it is a success
                    state.result = SharedRequestContextResult::Done { res: Ok(()), id };
                    return Poll::Ready(Ok(()));
                }
                SharedRequestContextResult::Done { res: Err(e), .. } => return Poll::Ready(Err(e)),
                r => state.result = r,
            }
            if state.state.header_finished {
                return Poll::Ready(Ok(()));
            }
            // Register the waker while holding the lock to avoid missing the wake-up signal
            self.shared_context.waker.register(cx.waker());
            Poll::Pending
        })
        .await?;

        let shared_context = self.shared_context.clone();
        let send_task_res = self
            .manager
            .clone()
            .dispatch_task(LoopTask::QueryHandleResponse(self));
        if send_task_res.is_err() {
            return Err(
                io::Error::new(io::ErrorKind::ConnectionAborted, "handle not found").into(),
            );
        }
        poll_fn(|cx| {
            let mut state = shared_context.state.lock().unwrap();
            if let Some(response) = state.response.take() {
                return Poll::Ready(Ok(response));
            }
            match std::mem::take(&mut state.result) {
                SharedRequestContextResult::Done { res: Err(e), .. } => return Poll::Ready(Err(e)),
                r => state.result = r,
            }
            shared_context.waker.register(cx.waker());
            Poll::Pending
        })
        .await
    }

    pub(super) async fn poll_bytes_async<T>(
        &mut self,
        cb: impl FnOnce(&mut Vec<u8>) -> nyquest_interface::Result<T>,
    ) -> nyquest_interface::Result<Option<T>> {
        let mut cb = Some(cb);
        poll_fn(|cx| {
            self.poll_bytes(cx, |buf| {
                let cb = cb
                    .take()
                    .expect("poll_bytes callback is called more than once");
                cb(buf)
            })
        })
        .await
    }
    pub(super) fn poll_bytes<T>(
        &mut self,
        cx: &mut Context<'_>,
        cb: impl FnOnce(&mut Vec<u8>) -> nyquest_interface::Result<T>,
    ) -> Poll<nyquest_interface::Result<Option<T>>> {
        let mut state = self.shared_context.state.lock().unwrap();
        if !state.state.response_buffer.is_empty() {
            let res = cb(&mut state.state.response_buffer);
            return Poll::Ready(res.map(Some));
        }
        if let SharedRequestContextResult::Done { res, .. } = std::mem::take(&mut state.result) {
            return Poll::Ready(res.map(|_| None));
        };
        self.manager
            .dispatch_task(LoopTask::UnpauseRecvHandle(self.id))
            .ok();
        self.shared_context.waker.register(cx.waker());
        Poll::Pending
    }
}

impl Drop for RequestHandle {
    fn drop(&mut self) {
        self.manager
            .dispatch_task(LoopTask::DropHandle(self.id))
            .ok();
    }
}

struct LoopManagerInner {
    tasks: VecDeque<LoopTask>,
    multi_waker: MultiWaker,
}

impl Drop for LoopManagerInner {
    fn drop(&mut self) {
        for mut lost_easy in self.tasks.drain(..).filter_map(|task| match task {
            LoopTask::ConstructHandle(easy) => Some(easy),
            _ => None,
        }) {
            let ctx = lost_easy.as_callback_mut().ctx.clone();
            ctx.state.lock().unwrap().result = SharedRequestContextResult::EasyLost(lost_easy);
            ctx.waker.wake();
        }
    }
}

#[derive(Clone)]
struct LoopManagerShared {
    inner: Weak<Mutex<LoopManagerInner>>,
}

pub(super) struct LoopManager {
    inner: FuturesMutex<Option<LoopManagerShared>>,
}

impl LoopManagerShared {
    async fn start_loop() -> Self {
        let (multi_waker_tx, multi_waker_rx) = oneshot::channel();
        thread::Builder::new()
            .name("nyquest-curl-multi-loop".into())
            .spawn(move || {
                run_loop(multi_waker_tx);
            })
            .expect("failed to spawn curl multi loop");
        multi_waker_rx.await.expect("not receiving request manager")
    }
    fn dispatch_task(&self, task: LoopTask) -> Result<(), LoopTask> {
        let Some(inner) = self.inner.upgrade() else {
            return Err(task);
        };
        let mut inner = inner.lock().unwrap();
        inner.tasks.push_back(task);
        inner.multi_waker.wakeup().ok();
        Ok(())
    }
    async fn start_request(
        &self,
        easy: Easy,
        shared_context: &SharedRequestContext,
    ) -> Result<usize, Easy> {
        {
            let Some(inner) = self.inner.upgrade() else {
                return Err(easy);
            };
            let mut inner = inner.lock().unwrap();
            if inner.multi_waker.wakeup().is_err() {
                drop(inner);
                return Err(easy);
            }
            let request = LoopTask::ConstructHandle(easy);
            inner.tasks.push_back(request);
        }
        poll_fn(|cx| {
            use SharedRequestContextResult::*;
            let mut state = shared_context.state.lock().unwrap();
            match std::mem::take(&mut state.result) {
                r @ InProgress { id } | r @ Done { id, .. } => {
                    state.result = r;
                    Poll::Ready(Ok(id))
                }
                EasyLost(easy) => Poll::Ready(Err(easy)),
                Init => {
                    shared_context.waker.register(cx.waker());
                    Poll::Pending
                }
            }
        })
        .await
    }
}

impl PartialEq for LoopManagerShared {
    fn eq(&self, other: &Self) -> bool {
        Weak::ptr_eq(&self.inner, &other.inner)
    }
}

impl Eq for LoopManagerShared {}

impl LoopManager {
    pub(super) fn new() -> Self {
        Self {
            inner: FuturesMutex::new(None),
        }
    }
    pub(super) async fn start_request(
        &self,
        mut easy: BoxEasyHandle<AsyncHandler>,
    ) -> nyquest_interface::Result<RequestHandle> {
        let shared_context = easy.as_callback_mut().ctx.clone();
        loop {
            let inner = match &mut *self.inner.lock().await {
                Some(inner) => inner.clone(),
                manager @ None => manager
                    .insert(LoopManagerShared::start_loop().await)
                    .clone(),
            };
            let backup_easy = match inner.start_request(easy, &shared_context).await {
                Ok(res) => {
                    return Ok(RequestHandle {
                        id: res,
                        shared_context,
                        manager: inner,
                    })
                }
                Err(res) => res,
            };
            {
                let mut new_manager = self.inner.lock().await;
                if *new_manager == Some(inner) {
                    *new_manager = Some(LoopManagerShared::start_loop().await);
                }
            }
            easy = backup_easy;
        }
    }
}

impl Drop for LoopManager {
    fn drop(&mut self) {
        if let Some(inner) = self.inner.get_mut() {
            inner.dispatch_task(LoopTask::Shutdown).ok();
        }
    }
}

fn run_loop(multl_waker_tx: oneshot::Sender<LoopManagerShared>) {
    let slab = SlabMultiSet::default();
    let multi = WakeableMulti::new(RawMulti::new());
    let multi_waker = multi.get_waker();
    let mut multi = MultiWithSet::new(multi, slab);
    let request_manager = Arc::new(Mutex::new(LoopManagerInner {
        tasks: Default::default(),
        multi_waker,
    }));
    if multl_waker_tx
        .send(LoopManagerShared {
            inner: Arc::downgrade(&request_manager),
        })
        .is_err()
    {
        return;
    }
    // TODO: store ctx in Easy2Handle
    let mut tasks = Default::default();
    let mut last_call = false;
    loop {
        let poll_res = multi.poll(120 * 1000);
        std::mem::swap(&mut request_manager.lock().unwrap().tasks, &mut tasks);
        for task in tasks.drain(..) {
            match task {
                LoopTask::ConstructHandle(mut easy) => {
                    let context = easy.as_callback_mut().ctx.clone();
                    let handle = multi.add(easy);
                    let mut state = context.state.lock().unwrap();
                    context.waker.wake();
                    match handle {
                        Ok(token) => {
                            state.result = SharedRequestContextResult::InProgress { id: token };
                        }
                        Err(e) => {
                            panic!("failed to add easy handle to multi: {e:?}");
                            // poison pill
                        }
                    };
                    last_call = false;
                }
                LoopTask::QueryHandleResponse(req_handle) => {
                    let id = req_handle.id;
                    let ctx = req_handle.shared_context.clone();
                    let mut state = ctx.state.lock().unwrap();
                    ctx.waker.wake();
                    let Some(handle) = multi.lookup(id) else {
                        state.result = SharedRequestContextResult::Done {
                            res: Err(io::Error::new(
                                io::ErrorKind::ConnectionAborted,
                                "handle not found",
                            )
                            .into()),
                            id,
                        };
                        continue;
                    };
                    let res = handle
                        .with_error_message(|mut e| {
                            let status = e.as_mut().as_raw_easy_mut().get_response_code()?;
                            let content_length = e
                                .as_mut()
                                .as_raw_easy_mut()
                                .get_content_length()
                                .unwrap_or_default()
                                .map(|l| l as _);
                            Ok(super::CurlAsyncResponse {
                                status,
                                content_length,
                                headers: state
                                    .state
                                    .response_headers_buffer
                                    .iter_mut()
                                    .filter_map(|line| std::str::from_utf8_mut(&mut *line).ok())
                                    .filter_map(|line| line.split_once(':'))
                                    .map(|(k, v)| (k.into(), v.trim_start().into()))
                                    .collect(),
                                handle: req_handle,
                                max_response_buffer_size: None, // To be filled in client.request()
                            })
                        })
                        .map_err(|e| e.into());
                    match res {
                        Ok(response) => {
                            state.response = Some(response);
                        }
                        Err(e) => {
                            state.result = SharedRequestContextResult::Done { res: Err(e), id };
                        }
                    };
                }
                LoopTask::UnpauseRecvHandle(id) => {
                    if let Some(easy) = multi.lookup(id) {
                        // Ignore the error. Also see
                        // https://github.com/sagebind/isahc/blob/9d1edd475231ad5cfd5842d939db1382dc3a88f5/src/agent/mod.rs#L432
                        easy.as_raw_easy_mut().unpause_recv().ok();
                    }
                }
                LoopTask::_UnpauseSendHandle(id) => {
                    if let Some(easy) = multi.lookup(id) {
                        // Ignore the error. Also see
                        // https://github.com/sagebind/isahc/blob/9d1edd475231ad5cfd5842d939db1382dc3a88f5/src/agent/mod.rs#L432
                        easy.as_raw_easy_mut().unpause_send().ok();
                    }
                }
                LoopTask::DropHandle(id) => {
                    multi.remove(id).ok();
                }
                LoopTask::Shutdown => {
                    // TODO: handle shutdown
                    last_call = true;
                }
            }
        }
        let perform_res = multi.perform();
        let loop_res = match (poll_res, perform_res) {
            (Ok(poll_res), Ok(perform_res)) => Ok((poll_res, perform_res)),
            (Err(e), _) | (_, Err(e)) => Err(e),
        };
        let (_poll_res, _perform_res) = match loop_res {
            Ok(res) => res,
            Err(err) => {
                for (id, ctx) in multi.iter_mut() {
                    let ctx = &*ctx.as_callback_mut().ctx;
                    let state = ctx.state.lock();
                    if let Ok(mut state) = state {
                        if !matches!(state.result, SharedRequestContextResult::Done { .. }) {
                            state.result = SharedRequestContextResult::Done {
                                res: Err(err.clone().into()),
                                id,
                            };
                            ctx.waker.wake();
                        }
                    }
                }
                break;
            }
        };
        multi.messages(|token, mut e, res| {
            let res = e
                .as_mut()
                .with_error_message(|_| res.transpose())
                .map_err(|e| e.into());
            let ctx = &*e.as_callback_mut().ctx;
            if let Some(res) = res.transpose() {
                let mut state = ctx.state.lock().unwrap();
                state.result = SharedRequestContextResult::Done { res, id: token };
            }
            ctx.waker.wake();
        });
        if multi.is_empty() {
            if last_call {
                break;
            }
            last_call = true;
        }

        multi.shrink_to_fit();
    }
}
