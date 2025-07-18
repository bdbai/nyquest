use std::collections::VecDeque;
use std::future::poll_fn;
use std::mem::ManuallyDrop;
use std::sync::{Arc, Mutex, Weak};
use std::task::{Context, Poll};
use std::time::Duration;
use std::{io, thread};

use curl::multi::{Multi, MultiWaker};
use futures_channel::oneshot;
use futures_util::lock::Mutex as FuturesMutex;
use futures_util::task::AtomicWaker;
use nyquest_interface::Result as NyquestResult;
use slab::Slab;

use crate::error::IntoNyquestResult;
use crate::share::{Share, ShareHandle};
use crate::state::RequestState;

type Easy2 = curl::easy::Easy2<super::handler::AsyncHandler>;
type Easy2Handle = curl::multi::Easy2Handle<super::handler::AsyncHandler>;

pub(super) struct RequestHandle {
    shared_context: Arc<SharedRequestContext>,
    manager: LoopManagerShared,
}

pub(super) struct SharedRequestContext {
    pub(super) id: usize,
    pub(super) waker: AtomicWaker,
    pub(super) state: Mutex<(RequestState, Option<NyquestResult<()>>)>,
}

impl SharedRequestContext {
    fn new(id: usize) -> Self {
        Self {
            id,
            waker: AtomicWaker::new(),
            state: Default::default(),
        }
    }
}

enum LoopTask {
    ConstructHandle(
        Easy2,
        oneshot::Sender<Result<NyquestResult<Arc<SharedRequestContext>>, Easy2>>,
    ),
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
            tx.send(Err(handle)).ok();
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
            .dispatch_task(LoopTask::QueryHandleResponse(
                self.shared_context.id,
                self,
                tx,
            ));
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
            .dispatch_task(LoopTask::UnpauseRecvHandle(self.shared_context.id))
            .ok();
        self.shared_context.waker.register(cx.waker());
        Poll::Pending
    }
}

impl Drop for RequestHandle {
    fn drop(&mut self) {
        self.manager
            .dispatch_task(LoopTask::DropHandle(self.shared_context.id))
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
    share: Share,
}

impl LoopManagerShared {
    async fn start_loop(share_handle: ShareHandle) -> Self {
        let (multi_waker_tx, multi_waker_rx) = oneshot::channel();
        thread::Builder::new()
            .name("nyquest-curl-multi-loop".into())
            .spawn(move || {
                let _share_handle = share_handle; // Ensure the handle outlives all easy handles in the loop
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
        easy: Easy2,
    ) -> NyquestResult<Result<RequestHandle, (Easy2, Self)>> {
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
        let shared_context = match rx.await {
            Ok(Ok(Ok(ctx))) => ctx,
            Ok(Ok(Err(e))) => return Err(e),
            Ok(Err(e)) => return Ok(Err((e, self))),
            Err(_) => unreachable!("Easy2 handle got lost"),
        };
        Ok(Ok(RequestHandle {
            shared_context,
            manager: self,
        }))
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
            share: Share::new(),
        }
    }
    pub(super) async fn start_request(
        &self,
        mut easy: Easy2,
    ) -> nyquest_interface::Result<RequestHandle> {
        unsafe {
            self.share
                .bind_easy2(&mut easy)
                .expect("failed to bind easy handle to share");
        }
        loop {
            let inner = match &mut *self.inner.lock().await {
                Some(inner) => inner.clone(),
                manager @ None => manager
                    .insert(LoopManagerShared::start_loop(self.share.get_handle()).await)
                    .clone(),
            };
            let (backup_easy, inner) = match inner.start_request(easy).await? {
                Ok(res) => return Ok(res),
                Err(res) => res,
            };
            {
                let mut new_manager = self.inner.lock().await;
                if *new_manager == Some(inner) {
                    *new_manager =
                        Some(LoopManagerShared::start_loop(self.share.get_handle()).await);
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
    let multi = Multi::new();
    let request_manager = Arc::new(Mutex::new(LoopManagerInner {
        tasks: Default::default(),
        multi_waker: multi.waker(),
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
    let mut slab = Slab::<(Easy2Handle, Arc<SharedRequestContext>)>::new();
    let mut tasks = Default::default();
    let mut last_call = false;
    loop {
        let poll_res = multi.poll(&mut [], Duration::from_secs(120));
        std::mem::swap(&mut request_manager.lock().unwrap().tasks, &mut tasks);
        for task in tasks.drain(..) {
            let mut task = ManuallyDrop::new(task);
            let mut task = unsafe { ManuallyDrop::take(&mut task.0) };
            loop {
                match task {
                    LoopTask::ConstructHandle(mut easy, tx) => {
                        let slab_entry = slab.vacant_entry();
                        let id = slab_entry.key();
                        let ctx = Arc::new(SharedRequestContext::new(id));
                        easy.get_mut().ctx = Some(ctx.clone());
                        let handle = multi
                            .add2(easy)
                            .into_nyquest_result("curl_multi_add_handle");
                        let send_res = match handle {
                            Ok(mut handle) => {
                                handle
                                    .set_token(id)
                                    .expect("failed to set token on easy handle");
                                slab_entry.insert((handle, ctx.clone()));
                                tx.send(Ok(Ok(ctx)))
                            }
                            Err(e) => {
                                tx.send(Ok(Err(e))).ok();
                                break;
                            }
                        };
                        if let Err(Ok(Ok(ctx))) = send_res {
                            task = LoopTask::DropHandle(ctx.id);
                            continue;
                        }
                        last_call = false;
                        break;
                    }
                    LoopTask::QueryHandleResponse(id, req_handle, tx) => {
                        let Some((handle, ctx)) = slab.get_mut(id) else {
                            break;
                        };
                        let mut state = ctx.state.lock().unwrap();
                        let res = handle
                            .response_code()
                            .map(|status| super::CurlAsyncResponse {
                                status: status as _,
                                content_length: handle
                                    .content_length_download()
                                    .ok()
                                    .map(|l| l as _),
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
                            });
                        tx.send(res.into_nyquest_result("get CURLINFO_RESPONSE_CODE"))
                            .ok();
                        break;
                    }
                    LoopTask::UnpauseRecvHandle(id) => {
                        if let Some((handle, _)) = slab.get(id) {
                            // Ignore the error. Also see
                            // https://github.com/sagebind/isahc/blob/9d1edd475231ad5cfd5842d939db1382dc3a88f5/src/agent/mod.rs#L432
                            handle.unpause_read().ok();
                        }
                    }
                    LoopTask::_UnpauseSendHandle(id) => {
                        if let Some((handle, _)) = slab.get(id) {
                            // Ignore the error. Also see
                            handle.unpause_write().ok();
                        }
                    }
                    LoopTask::DropHandle(id) => {
                        let (handle, _) = slab.remove(id);
                        let _ = multi.remove2(handle);
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
            (Err(poll_err), _) => Err((poll_err, "async loop curl_multi_poll")),
            (_, Err(perform_err)) => Err((perform_err, "async loop curl_multi_perform")),
        };
        let (_poll_res, _perform_res) = match loop_res {
            Ok(res) => res,
            Err((err, err_ctx)) => {
                for (_, (handle, ctx)) in slab {
                    if let Ok(mut state) = ctx.state.lock() {
                        if state.1.is_none() {
                            state.1 = Some(Err(err.clone()).into_nyquest_result(err_ctx));
                        }
                    }
                    multi.remove2(handle).ok();
                }
                break;
            }
        };
        // TODO: terminate the loop if the multi is empty after timeout
        multi.messages(|msg| {
            let Some((handle, ctx)) = msg.token().ok().and_then(|t| slab.get_mut(t)) else {
                return;
            };
            // TODO: handle message
            let Ok(mut shared_state) = ctx.state.lock() else {
                return;
            };
            if let Some(res) = msg.result_for2(handle) {
                shared_state.1 = Some(res.into_nyquest_result("curl_multi_info_read cb"));
            }
            drop(shared_state);
            ctx.waker.wake();
        });
        if slab.is_empty() {
            if last_call {
                break;
            }
            last_call = true;
        }

        slab.shrink_to_fit();
    }
}
