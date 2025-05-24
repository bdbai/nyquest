use std::ops::ControlFlow;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use curl::multi::Multi;
use nyquest_interface::blocking::Request;
use nyquest_interface::{Error as NyquestError, Result as NyquestResult};

use crate::error::IntoNyquestResult;
use crate::share::Share;

type Easy = curl::easy::Easy2<super::handler::BlockingHandler>;
type EasyHandle = curl::multi::Easy2Handle<super::handler::BlockingHandler>;

enum MaybeAttachedEasy {
    Attached(EasyHandle),
    Detached(Easy),
    Error(NyquestResult<()>),
}

pub(crate) struct MultiEasy {
    state: Arc<Mutex<MultiEasyState>>,
    easy: MaybeAttachedEasy,
    multi: Multi,
    share: Share, // Drop later than easy
}

#[derive(Default)]
pub(super) struct MultiEasyState {
    pub(super) temp_status_code: u16,
    pub(super) header_finished: bool,
    pub(super) response_headers_buffer: Vec<Vec<u8>>,
    pub(super) response_buffer: Vec<u8>,
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
                    *self = match multi.add2(easy) {
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
                    *self = match multi.remove2(easy) {
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
    pub fn new(share: Share) -> Self {
        let state = Arc::new(Mutex::new(MultiEasyState::default()));
        let easy = Easy::new(super::handler::BlockingHandler::new(state.clone()));
        let mut multi = Multi::new();
        multi.set_max_connects(5).expect("set max connects"); // Default of easy is 5
        MultiEasy {
            state,
            multi,
            easy: MaybeAttachedEasy::Detached(easy),
            share,
        }
    }

    pub fn reset_state(&mut self) {
        *self.state.lock().unwrap() = Default::default();
    }

    fn poll_until(
        &mut self,
        mut cb: impl FnMut(&Mutex<MultiEasyState>) -> NyquestResult<ControlFlow<()>>,
    ) -> NyquestResult<()> {
        let easy = self.easy.attach(&mut self.multi)?;
        // TODO: sigpipe
        loop {
            let suggested_timeout = self
                .multi
                .get_timeout()
                .into_nyquest_result("multi_easy curl_multi_timeout")?
                .unwrap_or(Duration::from_secs(1));
            let multi_res = self
                .multi
                .wait(&mut [], suggested_timeout)
                .into_nyquest_result("multi_easy curl_multi_wait");
            let multi_res = multi_res.and_then(|_| {
                self.multi
                    .perform()
                    .into_nyquest_result("multi_easy curl_multi_perform")
            })?;
            let mut res = ControlFlow::Continue(());
            self.multi.messages(|msg| match msg.result_for2(easy) {
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
    }

    pub fn poll_until_response_headers(&mut self) -> NyquestResult<()> {
        self.poll_until(|state| {
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
        unsafe { self.share.bind_easy2(easy)? };
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
        max_response_buffer_size: Option<u64>,
    ) -> NyquestResult<()> {
        self.poll_until(|state| {
            let Some(max_response_buffer_size) = max_response_buffer_size else {
                return Ok(ControlFlow::<()>::Continue(()));
            };
            let received_len = state.lock().unwrap().response_buffer.len();
            if received_len > max_response_buffer_size as usize {
                return Err(NyquestError::ResponseTooLarge);
            }
            Ok(ControlFlow::Continue(()))
        })
        .map(|_| ())
    }

    pub fn poll_until_partial_response(&mut self) -> NyquestResult<()> {
        self.poll_until(|state| {
            let is_empty = state.lock().unwrap().response_buffer.is_empty();
            Ok(if is_empty {
                ControlFlow::Continue(())
            } else {
                ControlFlow::Break(())
            })
        })
    }

    pub fn take_response_buffer(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.state.lock().unwrap().response_buffer)
    }

    pub fn with_response_buffer_mut<T>(&mut self, f: impl FnOnce(&mut Vec<u8>) -> T) -> T {
        f(&mut self.state.lock().unwrap().response_buffer)
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
