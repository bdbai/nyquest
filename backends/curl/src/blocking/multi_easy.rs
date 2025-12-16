use std::mem::ManuallyDrop;
use std::ops::ControlFlow;
use std::pin::Pin;

use nyquest_interface::blocking::Request;
use nyquest_interface::{Error as NyquestError, Result as NyquestResult};

use crate::blocking::handler::BlockingHandler;
use crate::curl_ng::easy::{AsRawEasyMut as _, Share};
use crate::curl_ng::mime::MimePartContent;
use crate::curl_ng::multi::{MultiWithSet, RawMulti};
use crate::request::{create_easy, AsCallbackMut as _, BoxEasyHandle, EasyHandle};
use crate::state::RequestState;

type Easy = EasyHandle<super::handler::BlockingHandler>;
type BoxEasy = BoxEasyHandle<super::handler::BlockingHandler>;
type Multi = MultiWithSet<RawMulti, super::set::SingleMultiSet>;

pub(crate) struct MultiEasy {
    multi: Multi,
}

impl MultiEasy {
    pub fn new(share: &Share) -> NyquestResult<Self> {
        let mut multi = Multi::new(RawMulti::new(), Default::default());
        let easy = create_easy::<BlockingHandler>(Default::default(), share)?;
        multi.add(easy)?;
        multi.set_max_connects(5).expect("set max connects"); // Default of easy is 5
        Ok(MultiEasy { multi })
    }

    fn with_detached_easy<T>(
        &mut self,
        f: impl FnOnce(Pin<&mut Easy>) -> NyquestResult<T>,
    ) -> NyquestResult<T> {
        let easy = self.multi.remove(0)?.expect("easy handle must exist");

        struct EasyGuard<'a> {
            easy: ManuallyDrop<BoxEasy>,
            multi: &'a mut Multi,
        }
        impl<'a> EasyGuard<'a> {
            fn restore(self) -> NyquestResult<()> {
                let mut this = ManuallyDrop::new(self);
                unsafe {
                    let easy = ManuallyDrop::take(&mut this.easy);
                    this.multi.add(easy)?;
                }
                Ok(())
            }
        }
        impl Drop for EasyGuard<'_> {
            fn drop(&mut self) {
                unsafe { self.multi.add(ManuallyDrop::take(&mut self.easy)) }.ok();
            }
        }
        let mut guard = EasyGuard {
            easy: ManuallyDrop::new(easy),
            multi: &mut self.multi,
        };
        let res = f(guard.easy.as_mut());
        guard.restore()?;
        res
    }

    fn easy_mut(&mut self) -> Pin<&mut Easy> {
        self.multi
            .lookup(0)
            .expect("easy handle must exist after adding")
    }

    pub fn reset_state(&mut self) -> NyquestResult<()> {
        self.with_detached_easy(reset_easy_state)
    }

    fn poll_until(
        &mut self,
        mut cb: impl FnMut(&mut RequestState) -> NyquestResult<ControlFlow<()>>,
    ) -> NyquestResult<()> {
        loop {
            let suggested_timeout = self.multi.get_timeout_ms()?.unwrap_or(1000);
            let multi_res = self.multi.wait(suggested_timeout);
            let multi_res = multi_res.and_then(|_| self.multi.perform())?;
            let mut res = ControlFlow::Continue(());
            self.multi.messages(|_, easy, msg| {
                let msg = easy.with_error_message(|_| msg.transpose());
                match msg {
                    Ok(Some(())) => res = ControlFlow::Break(Ok(())),
                    Ok(None) => {}
                    Err(err) => res = ControlFlow::Break(Err(NyquestError::from(err))),
                }
            });
            match res {
                ControlFlow::Break(res) => return res,
                ControlFlow::Continue(())
                    if multi_res == 0
                        || cb(&mut self.easy_mut().as_callback_mut().state)?.is_break() =>
                {
                    return Ok(())
                }
                _ => {}
            }
        }
    }

    pub fn poll_until_response_headers(&mut self) -> NyquestResult<()> {
        self.poll_until(|state| {
            Ok(if state.header_finished {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            })
        })
        .map(|_| ())
    }

    #[cfg(feature = "blocking-stream")]
    pub fn populate_request(
        &mut self,
        url: &str,
        req: Request,
        options: &nyquest_interface::client::ClientOptions,
    ) -> NyquestResult<()> {
        use nyquest_interface::blocking::BoxedStream;

        use crate::blocking::part_reader::BlockingPartReader;

        self.with_detached_easy(|easy| {
            crate::request::populate_request(
                url,
                req,
                options,
                easy,
                |mut easy, stream| {
                    if let BoxedStream::Sized { content_length, .. } = &stream {
                        easy.as_mut()
                            .as_raw_easy_mut()
                            .set_infile_size(*content_length)?;
                    }
                    easy.as_callback_mut().set_body_stream(stream);
                    Ok(())
                },
                |stream| {
                    let size = match &stream {
                        BoxedStream::Sized { content_length, .. } => Some(*content_length as i64),
                        BoxedStream::Unsized { .. } => None,
                    };
                    MimePartContent::Reader {
                        reader: BlockingPartReader::new(stream),
                        size,
                    }
                },
            )?;
            Ok(())
        })
    }

    #[cfg(not(feature = "blocking-stream"))]
    pub fn populate_request(
        &mut self,
        url: &str,
        req: Request,
        options: &nyquest_interface::client::ClientOptions,
    ) -> NyquestResult<()> {
        self.with_detached_easy(|easy| {
            use crate::curl_ng::mime::DummyMimePartReader;

            crate::request::populate_request(
                url,
                req,
                options,
                easy,
                |_, _| Ok(()),
                |_| -> MimePartContent<DummyMimePartReader> {
                    unreachable!("blocking-stream feature is disabled")
                },
            )?;
            Ok(())
        })
    }

    pub fn status(&mut self) -> NyquestResult<u16> {
        self.easy_mut()
            .with_error_message(|easy| easy.as_raw_easy_mut().get_response_code())
            .map_err(|e| e.into())
    }

    pub fn content_length(&mut self) -> NyquestResult<Option<u64>> {
        self.easy_mut()
            .with_error_message(|easy| easy.as_raw_easy_mut().get_content_length())
            .map_err(|e| e.into())
    }

    pub fn poll_until_whole_response(
        &mut self,
        max_response_buffer_size: Option<u64>,
    ) -> NyquestResult<()> {
        self.poll_until(|state| {
            let Some(max_response_buffer_size) = max_response_buffer_size else {
                return Ok(ControlFlow::<()>::Continue(()));
            };
            let received_len = state.response_buffer.len();
            if received_len > max_response_buffer_size as usize {
                return Err(NyquestError::ResponseTooLarge);
            }
            Ok(ControlFlow::Continue(()))
        })
        .map(|_| ())
    }

    #[cfg(feature = "blocking-stream")]
    pub fn poll_bytes<T>(&mut self, cb: impl FnOnce(&mut Vec<u8>) -> T) -> NyquestResult<T> {
        let mut easy = self.easy_mut();
        let buffer = &mut easy.as_callback_mut().state.response_buffer;
        if !buffer.is_empty() {
            return Ok(cb(buffer));
        }
        self.poll_until(|state| {
            let is_empty = state.response_buffer.is_empty();
            Ok(if is_empty {
                ControlFlow::Continue(())
            } else {
                ControlFlow::Break(())
            })
        })?;
        Ok(self.with_response_buffer_mut(cb))
    }

    pub fn take_response_buffer(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.easy_mut().as_callback_mut().state.response_buffer)
    }

    #[cfg(feature = "blocking-stream")]
    pub fn with_response_buffer_mut<T>(&mut self, f: impl FnOnce(&mut Vec<u8>) -> T) -> T {
        f(&mut self.easy_mut().as_callback_mut().state.response_buffer)
    }

    pub fn take_response_headers_buffer(&mut self) -> Vec<Vec<u8>> {
        std::mem::take(
            &mut self
                .easy_mut()
                .as_callback_mut()
                .state
                .response_headers_buffer,
        )
    }
}

fn reset_easy_state(easy: Pin<&mut Easy>) -> Result<(), NyquestError> {
    easy.with_error_message(|mut easy| {
        *easy.as_callback_mut() = Default::default();
        easy.reset()?;
        Ok(())
    })?;
    Ok(())
}
