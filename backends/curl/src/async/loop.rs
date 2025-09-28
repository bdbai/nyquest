use std::collections::VecDeque;
use std::future::poll_fn;
use std::mem::ManuallyDrop;
use std::sync::{Arc, Mutex, Weak};
use std::task::{Context, Poll};
use std::{io, thread};

use futures_channel::oneshot;
use futures_util::lock::Mutex as FuturesMutex;
use futures_util::task::AtomicWaker;
use nyquest_interface::{Error as NyquestError, Result as NyquestResult};

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
pub(super) struct SharedRequestContext {
    pub(super) waker: AtomicWaker,
    pub(super) state: Mutex<(RequestState, Option<NyquestResult<()>>)>,
}

enum ConstructHandleResponse {
    Success { id: usize },
    Error(NyquestError),
    Lost(Easy),
}

enum LoopTask {
    ConstructHandle(Easy, oneshot::Sender<ConstructHandleResponse>),
    QueryHandleResponse(
        usize,
        RequestHandle,
        oneshot::Sender<NyquestResult<super::CurlAsyncResponse>>,
    ),
    UnpauseRecvHandle(usize),
    _UnpauseSendHandle(usize),
    DropHandle(usize),
    Shutdown,
}

struct LoopTaskWrapper(ManuallyDrop<LoopTask>);

impl From<LoopTask> for LoopTaskWrapper {
    fn from(task: LoopTask) -> Self {
        Self(ManuallyDrop::new(task))
    }
}

impl Drop for LoopTaskWrapper {
    fn drop(&mut self) {
        let task = unsafe { ManuallyDrop::take(&mut self.0) };
        if let LoopTask::ConstructHandle(handle, tx) = task {
            tx.send(ConstructHandleResponse::Lost(handle)).ok();
        }
    }
}

impl RequestHandle {
    pub(super) async fn wait_for_response(
        self,
    ) -> nyquest_interface::Result<super::CurlAsyncResponse> {
        poll_fn(|cx| {
            let mut state = self.shared_context.state.lock().unwrap();
            if let Some(err_res) = state.1.take_if(|r| r.is_err()) {
                return Poll::Ready(err_res);
            }
            // Do not take out result if it is a success
            if state.1.is_some() || state.0.header_finished {
                return Poll::Ready(Ok(()));
            }
            // Register the waker while holding the lock to avoid missing the wake-up signal
            self.shared_context.waker.register(cx.waker());
            Poll::Pending
        })
        .await?;

        let (tx, rx) = oneshot::channel();
        let send_task_res = self
            .manager
            .clone()
            .dispatch_task(LoopTask::QueryHandleResponse(self.id, self, tx));
        let (Ok(_), Ok(res)) = (send_task_res, rx.await) else {
            return Err(
                io::Error::new(io::ErrorKind::ConnectionAborted, "handle not found").into(),
            );
        };
        res
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
        if !state.0.response_buffer.is_empty() {
            let res = cb(&mut state.0.response_buffer);
            return Poll::Ready(res.map(Some));
        }
        if let Some(res) = state.1.take() {
            return Poll::Ready(res.map(|()| None));
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
    tasks: VecDeque<LoopTaskWrapper>,
    multi_waker: MultiWaker,
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
        inner.tasks.push_back(task.into());
        inner.multi_waker.wakeup().ok();
        Ok(())
    }
    async fn start_request(
        self,
        mut easy: Easy,
    ) -> NyquestResult<Result<RequestHandle, (Easy, Self)>> {
        let shared_context = easy.as_callback_mut().ctx.clone();
        let (tx, rx) = oneshot::channel();
        {
            let Some(inner) = self.inner.upgrade() else {
                return Ok(Err((easy, self)));
            };
            let mut inner = inner.lock().unwrap();
            if inner.multi_waker.wakeup().is_err() {
                drop(inner);
                return Ok(Err((easy, self)));
            }
            let request = LoopTask::ConstructHandle(easy, tx);
            inner.tasks.push_back(request.into());
        }
        match rx.await {
            Ok(ConstructHandleResponse::Success { id }) => Ok(Ok(RequestHandle {
                id,
                shared_context,
                manager: self,
            })),
            Ok(ConstructHandleResponse::Error(e)) => Err(e),
            Ok(ConstructHandleResponse::Lost(handle)) => Ok(Err((handle, self))),
            Err(_) => unreachable!("Easy2 handle got lost"),
        }
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
        loop {
            let inner = match &mut *self.inner.lock().await {
                Some(inner) => inner.clone(),
                manager @ None => manager
                    .insert(LoopManagerShared::start_loop().await)
                    .clone(),
            };
            let (backup_easy, inner) = match inner.start_request(easy).await? {
                Ok(res) => return Ok(res),
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
            let mut task = ManuallyDrop::new(task);
            let mut task = unsafe { ManuallyDrop::take(&mut task.0) };
            loop {
                match task {
                    LoopTask::ConstructHandle(easy, tx) => {
                        let handle = multi.add(easy);
                        let send_res = match handle {
                            Ok(token) => tx.send(ConstructHandleResponse::Success { id: token }),
                            Err(e) => {
                                tx.send(ConstructHandleResponse::Error(e.into())).ok();
                                break;
                            }
                        };
                        if let Err(ConstructHandleResponse::Success { id }) = send_res {
                            task = LoopTask::DropHandle(id);
                            continue;
                        }
                        last_call = false;
                        break;
                    }
                    LoopTask::QueryHandleResponse(id, req_handle, tx) => {
                        let Some(handle) = multi.lookup(id) else {
                            break;
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
                                let mut state = e.as_callback_mut().ctx.state.lock().unwrap();
                                Ok(super::CurlAsyncResponse {
                                    status,
                                    content_length,
                                    headers: state
                                        .0
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
                        tx.send(res).ok();
                        break;
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
                break;
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
                for (_, ctx) in multi.iter_mut() {
                    let state = ctx.as_callback_mut().ctx.state.lock();
                    if let Ok(mut state) = state {
                        if state.1.is_none() {
                            state.1 = Some(Err(err.clone().into()));
                        }
                    }
                }
                break;
            }
        };
        multi.messages(|mut e, res| {
            let res = e
                .as_mut()
                .with_error_message(|_| res.transpose())
                .map_err(|e| e.into());
            let ctx = &*e.as_callback_mut().ctx;
            if let Some(res) = res.transpose() {
                let mut state = ctx.state.lock().unwrap();
                state.1 = Some(res);
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
