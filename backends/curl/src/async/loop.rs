use std::collections::VecDeque;
use std::future::poll_fn;
use std::sync::{Arc, Mutex};
use std::task::Poll;
use std::time::Duration;
use std::{io, thread};

use curl::easy::Easy;
use curl::multi::{EasyHandle, Multi, MultiWaker};
use curl_sys::{CURLPAUSE_RECV, CURLPAUSE_RECV_CONT, CURLPAUSE_SEND, CURLPAUSE_SEND_CONT};
use futures_channel::oneshot;
use futures_util::lock::Mutex as FuturesMutex;
use futures_util::task::AtomicWaker;
use nyquest_interface::Result as NyquestResult;
use slab::Slab;

use crate::error::IntoNyquestResult;

pub const CURLPAUSE_CONT: i32 = CURLPAUSE_RECV_CONT | CURLPAUSE_SEND_CONT;
pub const CURLPAUSE_ALL: i32 = CURLPAUSE_RECV | CURLPAUSE_SEND;

pub(super) struct RequestHandle {
    shared_context: Arc<SharedRequestContext>,
    manager: LoopManagerShared,
}

#[derive(Debug, Default)]
struct SharedRequestContextState {
    result: Option<NyquestResult<()>>,
    temp_status_code: u16,
    is_established: bool,
    header_finished: bool,
    response_headers_buffer: Vec<Vec<u8>>,
    response_buffer: Vec<u8>,
}
struct SharedRequestContext {
    id: usize,
    waker: AtomicWaker,
    state: Mutex<SharedRequestContextState>,
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
        Easy,
        oneshot::Sender<NyquestResult<Arc<SharedRequestContext>>>,
    ),
    QueryHandleResponse(
        usize,
        RequestHandle,
        oneshot::Sender<NyquestResult<super::CurlAsyncResponse>>,
    ),
    UnpauseHandle(usize),
    DropHandle(usize),
    Shutdown,
}

impl RequestHandle {
    pub(super) async fn wait_for_response(
        self,
    ) -> nyquest_interface::Result<super::CurlAsyncResponse> {
        poll_fn(|cx| {
            let mut state = self.shared_context.state.lock().unwrap();
            if let Some(err_res) = state.result.take_if(|r| r.is_err()) {
                return Poll::Ready(err_res);
            }
            // Do not take out result if it is a success
            if state.result.is_some() || state.header_finished {
                return Poll::Ready(Ok(()));
            }
            // Register the waker while holding the lock to avoid missing the wake-up signal
            self.shared_context.waker.register(cx.waker());
            Poll::Pending
        })
        .await?;

        let (tx, rx) = oneshot::channel();
        self.manager
            .clone()
            .dispatch_task(LoopTask::QueryHandleResponse(
                self.shared_context.id,
                self,
                tx,
            ));
        let Ok(res) = rx.await else {
            return Err(
                io::Error::new(io::ErrorKind::ConnectionAborted, "handle not found").into(),
            );
        };
        res
    }

    pub(super) async fn poll_bytes<T>(
        &mut self,
        cb: impl FnOnce(&mut Vec<u8>) -> nyquest_interface::Result<T>,
    ) -> nyquest_interface::Result<Option<T>> {
        self.manager
            .dispatch_task(LoopTask::UnpauseHandle(self.shared_context.id));
        let mut cb = Some(cb);
        poll_fn(|cx| {
            let mut state = self.shared_context.state.lock().unwrap();
            if !state.response_buffer.is_empty() {
                let cb = cb
                    .take()
                    .expect("poll_bytes callback is called more than once");
                let res = cb(&mut state.response_buffer);
                state.response_buffer.clear();
                return Poll::Ready(res.map(Some));
            }
            if let Some(res) = state.result.take() {
                return Poll::Ready(res.map(|()| None));
            };
            self.shared_context.waker.register(cx.waker());
            Poll::Pending
        })
        .await
    }
}

impl Drop for RequestHandle {
    fn drop(&mut self) {
        self.manager
            .dispatch_task(LoopTask::DropHandle(self.shared_context.id));
    }
}

struct LoopManagerInner {
    tasks: VecDeque<LoopTask>,
    multi_waker: MultiWaker,
}

#[derive(Clone)]
struct LoopManagerShared {
    inner: Arc<Mutex<LoopManagerInner>>,
}

pub(super) struct LoopManager {
    inner: FuturesMutex<Option<LoopManagerShared>>,
}

impl LoopManagerShared {
    async fn start_loop() -> Self {
        let (multi_waker_tx, multi_waker_rx) = oneshot::channel();
        thread::Builder::new()
            .name("nyquest-curl-multi-loop".into())
            .spawn(|| {
                run_loop(multi_waker_tx);
            })
            .expect("failed to spawn curl multi loop");
        multi_waker_rx.await.expect("not receiving request manager")
    }
    fn dispatch_task(&self, task: LoopTask) {
        let mut inner = self.inner.lock().unwrap();
        inner.tasks.push_back(task);
        inner.multi_waker.wakeup().ok();
    }
    async fn start_request(
        self,
        easy: Easy,
    ) -> NyquestResult<Result<RequestHandle, (Option<Easy>, Self)>> {
        let (tx, rx) = oneshot::channel();
        {
            let mut inner = self.inner.lock().unwrap();
            if inner.multi_waker.wakeup().is_err() {
                drop(inner);
                return Ok(Err((Some(easy), self)));
            }
            let request = LoopTask::ConstructHandle(easy, tx);
            inner.tasks.push_back(request);
        }
        let shared_context = match rx.await {
            Ok(Ok(ctx)) => ctx,
            Ok(Err(e)) => return Err(e),
            Err(_) => return Ok(Err((None, self))),
        };
        Ok(Ok(RequestHandle {
            shared_context,
            manager: self,
        }))
    }
}

impl PartialEq for LoopManagerShared {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }
}

impl Eq for LoopManagerShared {}

pub(super) enum MaybeStartedRequest {
    Started(RequestHandle),
    Gone,
}

impl LoopManager {
    pub(super) fn new() -> Self {
        Self {
            inner: FuturesMutex::new(None),
        }
    }
    pub(super) async fn start_request(
        &self,
        mut easy: Easy,
    ) -> nyquest_interface::Result<MaybeStartedRequest> {
        loop {
            let inner = match &mut *self.inner.lock().await {
                Some(inner) => inner.clone(),
                manager @ None => manager
                    .insert(LoopManagerShared::start_loop().await)
                    .clone(),
            };
            let (backup_easy, inner) = match inner.start_request(easy).await? {
                Ok(res) => return Ok(MaybeStartedRequest::Started(res)),
                Err(res) => res,
            };
            {
                let mut new_manager = self.inner.lock().await;
                if *new_manager == Some(inner) {
                    *new_manager = Some(LoopManagerShared::start_loop().await);
                }
            }
            match backup_easy {
                Some(e) => easy = e,
                None => {
                    return Ok(MaybeStartedRequest::Gone);
                }
            }
        }
    }
}

impl Drop for LoopManager {
    fn drop(&mut self) {
        if let Some(inner) = self.inner.get_mut() {
            inner.dispatch_task(LoopTask::Shutdown);
        }
    }
}

#[derive(Clone, Copy)]
struct EasyPause(*mut curl_sys::CURL);

impl EasyPause {
    fn new(handle: *mut curl_sys::CURL) -> Self {
        Self(handle)
    }

    /// ## Safety
    /// The caller must ensure:
    /// 1. The handle is a valid CURL handle.
    /// 2. The handle is either within the same thread or we are in a callback.
    unsafe fn pause(&self) {
        curl_sys::curl_easy_pause(self.0, CURLPAUSE_ALL);
    }
}

// Safety: Nothing can happen when the handle is moved between threads without "unsafe"
unsafe impl Send for EasyPause {}

fn run_loop(multl_waker_tx: oneshot::Sender<LoopManagerShared>) {
    let multi = Multi::new();
    let request_manager = LoopManagerShared {
        inner: Arc::new(Mutex::new(LoopManagerInner {
            tasks: Default::default(),
            multi_waker: multi.waker(),
        })),
    };
    if multl_waker_tx.send(request_manager.clone()).is_err() {
        return;
    }
    // TODO: store ctx in Easy2Handle
    let mut slab = Slab::<(EasyHandle, Arc<SharedRequestContext>)>::new();
    let mut tasks = Default::default();
    let mut last_call = false;
    loop {
        let poll_res = multi.poll(&mut [], Duration::from_secs(120));
        std::mem::swap(&mut request_manager.inner.lock().unwrap().tasks, &mut tasks);
        for mut task in tasks.drain(..) {
            loop {
                match task {
                    LoopTask::ConstructHandle(mut easy, tx) => {
                        let slab_entry = slab.vacant_entry();
                        let id = slab_entry.key();
                        let ctx = Arc::new(SharedRequestContext::new(id));
                        let pause = EasyPause::new(easy.raw());
                        easy.header_function({
                            let ctx = ctx.clone();
                            move |h| {
                                let mut state = ctx.state.lock().unwrap();
                                if h == b"\r\n" {
                                    let is_redirect =
                                        [301, 302, 303, 307, 308].contains(&state.temp_status_code);
                                    // TODO: handle direct
                                    if !is_redirect && !state.is_established {
                                        state.header_finished = true;
                                        unsafe {
                                            pause.pause();
                                        }
                                    }
                                } else if h.contains(&b':') {
                                    state
                                        .response_headers_buffer
                                        .push(h.strip_suffix(b"\r\n").unwrap_or(h).into());
                                } else {
                                    let mut status_components =
                                        h.splitn(3, u8::is_ascii_whitespace).skip(1);

                                    if let Some(status) = status_components
                                        .next()
                                        .and_then(|s| std::str::from_utf8(s).ok())
                                        .and_then(|s| s.parse().ok())
                                    {
                                        state.temp_status_code = status;
                                    }
                                    state.is_established = status_components
                                        .next()
                                        .map(|s| {
                                            s.eq_ignore_ascii_case(b"connection established\r\n")
                                        })
                                        .unwrap_or(false);
                                }
                                drop(state);
                                ctx.waker.wake();
                                true
                            }
                        })
                        .expect("set curl header function");
                        easy.write_function({
                            let ctx = ctx.clone();
                            move |f| {
                                let mut state = ctx.state.lock().unwrap();
                                state.header_finished = true;
                                // TODO: handle max response buffer size
                                state.response_buffer.extend_from_slice(f);
                                drop(state);
                                ctx.waker.wake();
                                Ok(f.len())
                            }
                        })
                        .expect("set curl write function");
                        let handle = multi.add(easy).into_nyquest_result("curl_multi_add_handle");
                        let send_res = match handle {
                            Ok(mut handle) => {
                                handle
                                    .set_token(id)
                                    .expect("failed to set token on easy handle");
                                slab_entry.insert((handle, ctx.clone()));
                                tx.send(Ok(ctx))
                            }
                            Err(e) => {
                                tx.send(Err(e)).ok();
                                break;
                            }
                        };
                        if let Err(Ok(ctx)) = send_res {
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
                                    .response_headers_buffer
                                    .iter_mut()
                                    .filter_map(|line| std::str::from_utf8_mut(&mut *line).ok())
                                    .filter_map(|line| line.split_once(':'))
                                    .map(|(k, v)| (k.into(), v.trim_start().into()))
                                    .collect(),
                                handle: req_handle,
                            });
                        tx.send(res.into_nyquest_result("get CURLINFO_RESPONSE_CODE"))
                            .ok();
                        break;
                    }
                    LoopTask::UnpauseHandle(id) => {
                        if let Some((handle, _)) = slab.get(id) {
                            unsafe {
                                let _res = curl_sys::curl_easy_pause(handle.raw(), CURLPAUSE_CONT);
                                // Ignore the error. Also see
                                // https://github.com/sagebind/isahc/blob/9d1edd475231ad5cfd5842d939db1382dc3a88f5/src/agent/mod.rs#L432
                            }
                        }
                    }
                    LoopTask::DropHandle(id) => {
                        let (handle, _) = slab.remove(id);
                        let _ = multi.remove(handle);
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
                        if state.result.is_none() {
                            state.result = Some(Err(err.clone()).into_nyquest_result(err_ctx));
                        }
                    }
                    multi.remove(handle).ok();
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
            if let Some(res) = msg.result_for(handle) {
                shared_state.result = Some(res.into_nyquest_result("curl_multi_info_read cb"));
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
