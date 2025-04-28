use std::ops::ControlFlow;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use curl::{
    easy::Easy,
    multi::{EasyHandle, Multi},
};
use nyquest_interface::blocking::Request;
use nyquest_interface::{Error as NyquestError, Result as NyquestResult};

use crate::error::IntoNyquestResult;
use crate::share::{Share, ShareHandle};

enum MaybeAttachedEasy {
    Attached(EasyHandle),
    Detached(Easy),
    Error(NyquestResult<()>),
}

pub(crate) struct MultiEasy {
    state: Arc<Mutex<MultiEasyState>>,
    easy: MaybeAttachedEasy,
    multi: Multi,
    _share_handle: ShareHandle,
}

#[derive(Default)]
struct MultiEasyState {
    temp_status_code: u16,
    header_finished: bool,
    response_headers_buffer: Vec<Vec<u8>>,
    response_buffer: Vec<u8>,
}

impl MaybeAttachedEasy {
    fn attach(&mut self, multi: &mut Multi) -> NyquestResult<&mut EasyHandle> {
        loop {
            match self {
                MaybeAttachedEasy::Attached(handle) => return Ok(handle),
                MaybeAttachedEasy::Detached(_) => {
                    let this = std::mem::replace(self, MaybeAttachedEasy::Error(Ok(())));
                    let easy = match this {
                        MaybeAttachedEasy::Detached(easy) => easy,
                        _ => unsafe {
                            // Safety: we just matched on `this` which is `MaybeAttachedEasy::Detached`.
                            std::hint::unreachable_unchecked();
                        },
                    };
                    *self = match multi.add(easy) {
                        Ok(handle) => MaybeAttachedEasy::Attached(handle),
                        Err(err) => MaybeAttachedEasy::Error(
                            Err(err).into_nyquest_result("multi_easy curl_multi_add_handle"),
                        ),
                    };
                    continue;
                }
                MaybeAttachedEasy::Error(err) => {
                    return std::mem::replace(err, Ok(())).map(|()| unreachable!());
                }
            };
        }
    }
    fn detach(&mut self, multi: &mut Multi) -> NyquestResult<&mut Easy> {
        loop {
            match self {
                MaybeAttachedEasy::Attached(_) => {
                    let this = std::mem::replace(self, MaybeAttachedEasy::Error(Ok(())));
                    let easy = match this {
                        MaybeAttachedEasy::Attached(easy) => easy,
                        _ => unsafe {
                            // Safety: we just matched on `this` which is `MaybeAttachedEasy::Attached`.
                            std::hint::unreachable_unchecked();
                        },
                    };
                    *self = match multi.remove(easy) {
                        Ok(easy) => MaybeAttachedEasy::Detached(easy),
                        Err(err) => MaybeAttachedEasy::Error(
                            Err(err).into_nyquest_result("multi_easy curl_multi_remove_handle"),
                        ),
                    };
                    continue;
                }
                MaybeAttachedEasy::Detached(easy) => return Ok(easy),
                MaybeAttachedEasy::Error(err) => {
                    return std::mem::replace(err, Ok(())).map(|()| unreachable!());
                }
            }
        }
    }
}

impl MultiEasy {
    pub fn new(share: &Share) -> Self {
        let state = Arc::new(Mutex::new(MultiEasyState::default()));
        let share_handle = share.get_handle(); // Drop later than easy
        let mut easy = Easy::new();
        unsafe { share.bind_easy(&mut easy) }.expect("bind easy to share");
        easy.header_function({
            let state = state.clone();
            move |h| {
                let mut state = state.lock().unwrap();
                if h == b"\r\n" {
                    let is_redirect = [301, 302, 303, 307, 308].contains(&state.temp_status_code);
                    if !is_redirect {
                        state.header_finished = true;
                    }
                } else if h.contains(&b':') {
                    state
                        .response_headers_buffer
                        .push(h.strip_suffix(b"\r\n").unwrap_or(h).into());
                } else if let Some(status) = h
                    .split(u8::is_ascii_whitespace)
                    .nth(1)
                    .and_then(|s| std::str::from_utf8(s).ok())
                    .and_then(|s| s.parse().ok())
                {
                    state.temp_status_code = status;
                }

                true
            }
        })
        .expect("set curl header function");
        easy.write_function({
            let state = state.clone();
            move |f| {
                let mut state = state.lock().unwrap();
                state.header_finished = true;
                // TODO: handle max response buffer size
                state.response_buffer.extend_from_slice(f);
                Ok(f.len())
            }
        })
        .expect("set curl write function");
        let mut multi = Multi::new();
        multi.set_max_connects(5).expect("set max connects"); // Default of easy is 5
        MultiEasy {
            state,
            multi,
            easy: MaybeAttachedEasy::Detached(easy),
            _share_handle: share_handle,
        }
    }

    pub fn reset_state(&mut self) {
        *self.state.lock().unwrap() = Default::default();
    }

    fn poll_until(
        &mut self,
        timeout: Duration,
        mut cb: impl FnMut(&Mutex<MultiEasyState>) -> NyquestResult<ControlFlow<()>>,
    ) -> NyquestResult<()> {
        let easy = self.easy.attach(&mut self.multi)?;
        let deadline = Instant::now() + timeout;
        // TODO: sigpipe
        while Instant::now() < deadline {
            let multi_res = self
                .multi
                .wait(&mut [], Duration::from_secs(1))
                .into_nyquest_result("multi_easy curl_multi_wait"); // TODO: proper timeout per wait
            let multi_res = multi_res.and_then(|_| {
                self.multi
                    .perform()
                    .into_nyquest_result("multi_easy curl_multi_perform")
            })?;
            let mut res = ControlFlow::Continue(());
            self.multi.messages(|msg| match msg.result_for(easy) {
                Some(Ok(())) => res = ControlFlow::Break(Ok(())),
                Some(Err(err)) => {
                    res = ControlFlow::Break(
                        Err(err).into_nyquest_result("multi_easy curl_multi_info_read cb"),
                    )
                }
                None => {}
            });
            match res {
                ControlFlow::Break(res) => return res,
                ControlFlow::Continue(()) if multi_res == 0 || cb(&self.state)?.is_break() => {
                    return Ok(())
                }
                _ => {}
            }
        }
        Err(curl::Error::new(curl_sys::CURLE_OPERATION_TIMEDOUT))
            .into_nyquest_result("multi_easy poll_until")
    }

    pub fn poll_until_response_headers(&mut self, timeout: Duration) -> NyquestResult<()> {
        self.poll_until(timeout, |state| {
            Ok(if state.lock().unwrap().header_finished {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            })
        })
        .map(|_| ())
    }

    pub fn populate_request(
        &mut self,
        url: &str,
        req: Request,
        options: &nyquest_interface::client::ClientOptions,
    ) -> NyquestResult<()> {
        self.reset_state();
        let easy = self.easy.detach(&mut self.multi)?;
        easy.reset();
        *self.state.lock().unwrap() = Default::default();
        crate::request::populate_request(url, &req, options, easy)
    }

    pub fn status(&mut self) -> NyquestResult<u16> {
        Ok(self.state.lock().unwrap().temp_status_code)
    }

    pub fn content_length(&mut self) -> NyquestResult<Option<u64>> {
        let content_length = match &mut self.easy {
            MaybeAttachedEasy::Attached(handle) => handle.content_length_download().ok(),
            MaybeAttachedEasy::Detached(handle) => handle.content_length_download().ok(),
            MaybeAttachedEasy::Error(_) => None,
        };
        Ok(content_length.map(|len| len as u64))
    }

    pub fn poll_until_whole_response(
        &mut self,
        timeout: Duration,
        max_response_buffer_size: Option<u64>,
    ) -> NyquestResult<()> {
        self.poll_until(timeout, |state| {
            let Some(max_response_buffer_size) = max_response_buffer_size else {
                return Ok(ControlFlow::Continue(()));
            };
            let received_len = state.lock().unwrap().response_buffer.len();
            if received_len > max_response_buffer_size as usize {
                return Err(NyquestError::ResponseTooLarge);
            }
            Ok(ControlFlow::Continue(()))
        })
    }

    pub fn take_response_buffer(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.state.lock().unwrap().response_buffer)
    }

    pub fn take_response_headers_buffer(&mut self) -> Vec<Vec<u8>> {
        std::mem::take(&mut self.state.lock().unwrap().response_headers_buffer)
    }
}

// Safety: `Multi` is not `Send` because the behavior of a `Easy` handle and its corresponding
// `Multi` handle dropped on different threads is not guaranteed.
// See https://github.com/alexcrichton/curl-rust/pull/213.
// However, `MultiEasy` can be `Send` because it bundles both `Easy` and `Multi` handles together,
// ensuring that they are dropped on the same thread. Moreover, we intentionally do not expose
// `&mut Easy` or `&mut Multi` to the user, so the user cannot move them to another thread.
unsafe impl Send for MultiEasy where Arc<MultiEasyState>: Send {}
