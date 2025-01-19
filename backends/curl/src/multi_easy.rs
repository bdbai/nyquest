use std::io::{self, ErrorKind};
use std::ops::ControlFlow;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use curl::easy::List;
use curl::{
    easy::Easy,
    multi::{EasyHandle, Multi},
    MultiError,
};
use curl_sys::CURLM_OK;
use nyquest::blocking::{Body, Request};

use crate::error::IntoNyquestResult;

type CurlResult<T> = Result<T, curl::Error>;

enum MaybeAttachedEasy {
    Attached(EasyHandle),
    Detached(Easy),
    Error(MultiError),
}

pub(crate) struct MultiEasy {
    state: Arc<Mutex<MultiEasyState>>,
    easy: MaybeAttachedEasy,
    multi: Multi,
}

#[derive(Default)]
struct MultiEasyState {
    header_finished: bool,
    response_headers_buffer: Vec<Vec<u8>>,
    response_buffer: Vec<u8>,
}

impl MaybeAttachedEasy {
    fn attach(&mut self, multi: &mut Multi) -> Result<&mut EasyHandle, MultiError> {
        loop {
            match self {
                MaybeAttachedEasy::Attached(handle) => return Ok(handle),
                MaybeAttachedEasy::Detached(_) => {
                    let this = std::mem::replace(
                        self,
                        MaybeAttachedEasy::Error(MultiError::new(CURLM_OK)),
                    );
                    let easy = match this {
                        MaybeAttachedEasy::Detached(easy) => easy,
                        _ => unsafe {
                            // Safety: we just matched on `this` which is `MaybeAttachedEasy::Detached`.
                            std::hint::unreachable_unchecked();
                        },
                    };
                    *self = match multi.add(easy) {
                        Ok(handle) => MaybeAttachedEasy::Attached(handle),
                        Err(err) => MaybeAttachedEasy::Error(err),
                    };
                    continue;
                }
                MaybeAttachedEasy::Error(err) => return Err(err.clone()),
            };
        }
    }
    fn detach(&mut self, multi: &mut Multi) -> Result<&mut Easy, MultiError> {
        loop {
            match self {
                MaybeAttachedEasy::Attached(_) => {
                    let this = std::mem::replace(
                        self,
                        MaybeAttachedEasy::Error(MultiError::new(CURLM_OK)),
                    );
                    let easy = match this {
                        MaybeAttachedEasy::Attached(easy) => easy,
                        _ => unsafe {
                            // Safety: we just matched on `this` which is `MaybeAttachedEasy::Attached`.
                            std::hint::unreachable_unchecked();
                        },
                    };
                    *self = match multi.remove(easy) {
                        Ok(easy) => MaybeAttachedEasy::Detached(easy),
                        Err(err) => MaybeAttachedEasy::Error(err),
                    };
                    continue;
                }
                MaybeAttachedEasy::Detached(easy) => return Ok(easy),
                MaybeAttachedEasy::Error(err) => return Err(err.clone()),
            }
        }
    }
}

impl MultiEasy {
    pub fn new() -> Self {
        let state = Arc::new(Mutex::new(MultiEasyState::default()));
        let mut easy = Easy::new();
        easy.header_function({
            let state = state.clone();
            move |h| {
                let mut state = state.lock().unwrap();
                if h == b"\r\n" {
                    state.header_finished = true;
                } else if h.contains(&b':') {
                    state
                        .response_headers_buffer
                        .push(h.strip_suffix(b"\r\n").unwrap_or(h).into());
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
        }
    }

    fn poll_until(
        &mut self,
        timeout: Duration,
        mut cb: impl FnMut(&Mutex<MultiEasyState>) -> CurlResult<ControlFlow<()>>,
    ) -> nyquest::Result<()> {
        let easy = self.easy.attach(&mut self.multi).into_nyquest_result()?;
        let deadline = Instant::now() + timeout;
        // TODO: sigpipe
        while Instant::now() < deadline {
            let mut multi_res = self.multi.wait(&mut [], Duration::from_secs(1)); // TODO: proper timeout per wait
            multi_res = multi_res.and_then(|_| self.multi.perform());
            let multi_res = match multi_res {
                Ok(res) => res,
                Err(err) => {
                    return Err(err).into_nyquest_result();
                }
            };
            let mut res = ControlFlow::Continue(());
            self.multi.messages(|msg| match msg.result_for(easy) {
                Some(Ok(())) => res = ControlFlow::Break(Ok(())),
                Some(Err(err)) => res = ControlFlow::Break(Err(err)),
                None => {}
            });
            match res {
                ControlFlow::Break(res) => return res.into_nyquest_result(),
                ControlFlow::Continue(())
                    if multi_res == 0 || cb(&self.state).into_nyquest_result()?.is_break() =>
                {
                    return Ok(())
                }
                _ => {}
            }
        }
        Err(curl::Error::new(curl_sys::CURLE_OPERATION_TIMEDOUT)).into_nyquest_result()
    }

    pub fn poll_until_response_headers(&mut self, timeout: Duration) -> nyquest::Result<()> {
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
        options: &nyquest::client::ClientOptions,
    ) -> nyquest::Result<()> {
        let easy = self.easy.detach(&mut self.multi).into_nyquest_result()?;
        easy.reset();
        *self.state.lock().unwrap() = Default::default();
        if let Some(user_agent) = options.user_agent.as_deref() {
            easy.useragent(&user_agent).expect("set curl user agent");
        }
        easy.url(url).into_nyquest_result()?;
        match &*req.method {
            "GET" | "get" => easy.get(true),
            "POST" | "post" => easy.post(true),
            "PUT" | "put" => easy.put(true),
            method => easy.custom_request(method),
        }
        .into_nyquest_result()?;
        let mut headers = List::new();
        for (name, value) in req.additional_headers {
            headers
                .append(&format!("{}: {}", name, value))
                .into_nyquest_result()?;
        }
        match req.body {
            Some(Body::Bytes {
                content,
                content_type,
            }) => {
                headers
                    .append(&format!("content-type: {}", content_type))
                    .into_nyquest_result()?;
                easy.post_fields_copy(&*content).into_nyquest_result()?;
            }
            Some(Body::Stream(_)) => unimplemented!(),
            Some(Body::Form { fields }) => {
                let mut form = curl::easy::Form::new();
                for (name, value) in fields {
                    form.part(&name)
                        .contents(value.as_bytes())
                        .add()
                        .map_err(|e| {
                            nyquest::Error::Io(io::Error::new(ErrorKind::Other, e.to_string()))
                        })?;
                }
                easy.httppost(form).into_nyquest_result()?;
            }
            Some(Body::Multipart { parts: _ }) => unimplemented!("mime is not ready in curl crate"),
            None => {}
        }
        easy.http_headers(headers).into_nyquest_result()?;
        Ok(())
    }

    pub fn status(&mut self) -> nyquest::Result<u16> {
        let status = match &mut self.easy {
            MaybeAttachedEasy::Attached(handle) => handle.response_code().into_nyquest_result(),
            MaybeAttachedEasy::Detached(handle) => handle.response_code().into_nyquest_result(),
            MaybeAttachedEasy::Error(err) => Err(err.clone()).into_nyquest_result(),
        }?;
        Ok(status as u16)
    }

    pub fn content_length(&mut self) -> nyquest::Result<Option<u64>> {
        let content_length = match &mut self.easy {
            MaybeAttachedEasy::Attached(handle) => handle.content_length_download().ok(),
            MaybeAttachedEasy::Detached(handle) => handle.content_length_download().ok(),
            MaybeAttachedEasy::Error(_) => None,
        };
        Ok(content_length.map(|len| len as u64))
    }

    pub fn poll_until_whole_response(&mut self, timeout: Duration) -> nyquest::Result<()> {
        self.poll_until(timeout, |_| Ok(ControlFlow::Continue(())))
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
