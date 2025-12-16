#[cfg(feature = "blocking-stream")]
use std::io;
use std::mem::ManuallyDrop;
use std::sync::{Arc, Mutex};

use nyquest_interface::blocking::Request;
use nyquest_interface::{Error as NyquestError, Result as NyquestResult};

mod handler;
mod multi_easy;
#[cfg(feature = "blocking-stream")]
mod part_reader;
mod set;

use crate::curl_ng::easy::Share;
use crate::url::concat_url;
use multi_easy::MultiEasy;

#[derive(Clone)]
pub struct CurlEasyClient {
    options: Arc<nyquest_interface::client::ClientOptions>,
    slot: Arc<MultiEasySlot>,
    share: Share,
}

struct MultiEasySlot {
    multi_easy: Mutex<Option<MultiEasy>>,
}

struct EasyHandleGuard<S: AsRef<MultiEasySlot>> {
    slot: S,
    handle: ManuallyDrop<Mutex<MultiEasy>>, // TODO: use std::sync::Exclusive when stabilized
}

type OwnedEasyHandleGuard = EasyHandleGuard<Arc<MultiEasySlot>>;

pub struct CurlBlockingResponse {
    status: u16,
    content_length: Option<u64>,
    headers: Vec<(String, String)>,
    handle: OwnedEasyHandleGuard,
    max_response_buffer_size: Option<u64>,
}

impl<S: AsRef<MultiEasySlot>> EasyHandleGuard<S> {
    fn handle_mut(&mut self) -> &mut MultiEasy {
        self.handle.get_mut().unwrap()
    }
}

impl EasyHandleGuard<&'_ Arc<MultiEasySlot>> {
    fn into_owned(self) -> OwnedEasyHandleGuard {
        let mut this = ManuallyDrop::new(self);
        // Safety: self inside ManuallyDrop will not be dropped, hence the handle will not be taken out from Drop
        let handle = unsafe { ManuallyDrop::take(&mut this.handle) };
        EasyHandleGuard {
            slot: this.slot.clone(),
            handle: ManuallyDrop::new(handle),
        }
    }
}

impl<S: AsRef<MultiEasySlot>> Drop for EasyHandleGuard<S> {
    fn drop(&mut self) {
        // Safety: the handle is only taken out once which is here, except in `into_owned` where a `ManuallyDrop` is
        // used to suppress our Drop
        let mut handle = unsafe { ManuallyDrop::take(&mut self.handle) };
        let Ok(mut slot) = self.slot.as_ref().multi_easy.try_lock() else {
            // Another thread will be putting back their handle, so we just give up.
            return;
        };
        if slot.is_none() && handle.get_mut().unwrap().reset_state().is_ok() {
            *slot = Some(handle.into_inner().unwrap());
        }
    }
}

impl CurlEasyClient {
    pub fn new(options: nyquest_interface::client::ClientOptions) -> Self {
        Self {
            options: Arc::new(options),
            slot: Arc::new(MultiEasySlot {
                multi_easy: Mutex::new(None),
            }),
            share: Share::new(),
        }
    }

    fn get_or_create_handle(&self) -> NyquestResult<EasyHandleGuard<&Arc<MultiEasySlot>>> {
        let slot = {
            let mut slot = self.slot.multi_easy.lock().unwrap();
            slot.take()
        };
        let handle = match slot {
            Some(handle) => handle,
            None => MultiEasy::new(&self.share)?,
        };
        Ok(EasyHandleGuard {
            slot: &self.slot,
            handle: ManuallyDrop::new(Mutex::new(handle)),
        })
    }
}

#[cfg(feature = "blocking-stream")]
impl io::Read for CurlBlockingResponse {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let handle = self.handle.handle_mut();
        let res = handle.poll_bytes(|response_buf| {
            let len = response_buf.len().min(buf.len());
            buf[..len].copy_from_slice(&response_buf[..len]);
            response_buf.drain(..len);
            len
        });
        match res {
            Ok(len) => Ok(len),
            Err(NyquestError::RequestTimeout) => Err(io::ErrorKind::TimedOut.into()),
            Err(NyquestError::Io(e)) => Err(e),
            Err(e) => unreachable!("Unexpected error: {}", e),
        }
    }
}

impl nyquest_interface::blocking::BlockingResponse for CurlBlockingResponse {
    fn status(&self) -> u16 {
        self.status
    }

    fn content_length(&self) -> Option<u64> {
        self.content_length
    }

    fn get_header(&self, header: &str) -> nyquest_interface::Result<Vec<String>> {
        Ok(self
            .headers
            .iter()
            .filter(|(k, _)| k.eq_ignore_ascii_case(header))
            .map(|(_, v)| v.clone())
            .collect())
    }

    fn text(&mut self) -> nyquest_interface::Result<String> {
        let buf = self.bytes()?;
        #[cfg(feature = "charset")]
        if let Some((_, mut charset)) = self
            .get_header("content-type")?
            .pop()
            .unwrap_or_default()
            .split(';')
            .filter_map(|s| s.split_once('='))
            .find(|(k, _)| k.trim().eq_ignore_ascii_case("charset"))
        {
            charset = charset.trim_matches('"');
            if let Ok(decoded) = iconv_native::decode_lossy(&buf, charset.trim()) {
                return Ok(decoded);
            }
        }
        Ok(String::from_utf8_lossy(&buf).into_owned())
    }

    fn bytes(&mut self) -> nyquest_interface::Result<Vec<u8>> {
        let handle = self.handle.handle_mut();
        handle.poll_until_whole_response(self.max_response_buffer_size)?;
        let buf = handle.take_response_buffer();
        if self
            .max_response_buffer_size
            .map(|limit| buf.len() > limit as usize)
            .unwrap_or_default()
        {
            return Err(NyquestError::ResponseTooLarge);
        }
        Ok(buf)
    }
}

impl nyquest_interface::blocking::BlockingClient for CurlEasyClient {
    type Response = CurlBlockingResponse;

    fn request(&self, req: Request) -> nyquest_interface::Result<Self::Response> {
        let mut handle_guard = self.get_or_create_handle()?;
        // FIXME: properly concat base_url and url
        let url = concat_url(self.options.base_url.as_deref(), &req.relative_uri);
        let handle: &mut MultiEasy = handle_guard.handle_mut();
        handle.populate_request(&url, req, &self.options)?;
        handle.poll_until_response_headers()?;
        let mut headers_buf = handle.take_response_headers_buffer();
        let headers = headers_buf
            .iter_mut()
            .filter_map(|line| std::str::from_utf8_mut(&mut *line).ok())
            .filter_map(|line| line.split_once(':'))
            .map(|(k, v)| (k.into(), v.trim_start().into()))
            .collect();
        Ok(CurlBlockingResponse {
            status: handle.status()?,
            content_length: handle.content_length()?,
            headers,
            handle: handle_guard.into_owned(),
            max_response_buffer_size: self.options.max_response_buffer_size,
        })
    }
}

impl nyquest_interface::blocking::BlockingBackend for crate::CurlBackend {
    type BlockingClient = CurlEasyClient;

    fn create_blocking_client(
        &self,
        options: nyquest_interface::client::ClientOptions,
    ) -> NyquestResult<Self::BlockingClient> {
        Ok(CurlEasyClient::new(options))
    }
}
