use std::io;
use std::mem::ManuallyDrop;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use nyquest::blocking::Request;
use nyquest::Error as NyquestError;

use crate::multi_easy::MultiEasy;
use crate::url::concat_url;

#[derive(Clone)]
pub struct CurlEasyClient {
    options: Arc<nyquest::client::ClientOptions>,
    slot: Arc<Mutex<Option<MultiEasy>>>,
}

struct EasyHandleGuard<S: AsRef<Mutex<Option<MultiEasy>>>> {
    slot: S,
    handle: ManuallyDrop<Mutex<MultiEasy>>, // TODO: use std::sync::Exclusive when stabilized
}

type OwnedEasyHandleGuard = EasyHandleGuard<Arc<Mutex<Option<MultiEasy>>>>;

pub struct CurlResponse {
    status: u16,
    content_length: Option<u64>,
    headers: Vec<(String, String)>,
    handle: OwnedEasyHandleGuard,
}

impl<S: AsRef<Mutex<Option<MultiEasy>>>> EasyHandleGuard<S> {
    fn with_handle<T>(&mut self, cb: impl FnOnce(&mut MultiEasy) -> T) -> T {
        cb(&mut self.handle.get_mut().unwrap())
    }
}

impl EasyHandleGuard<&'_ Arc<Mutex<Option<MultiEasy>>>> {
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

impl<S: AsRef<Mutex<Option<MultiEasy>>>> Drop for EasyHandleGuard<S> {
    fn drop(&mut self) {
        // Safety: the handle is only taken out once which is here, except in `into_owned` where a `ManuallyDrop` is
        // used to suppress our Drop
        let handle = unsafe { ManuallyDrop::take(&mut self.handle) };
        let mut slot = self.slot.as_ref().lock().unwrap();
        if slot.is_none() {
            *slot = Some(handle.into_inner().unwrap());
        }
    }
}

impl CurlEasyClient {
    pub fn new(options: nyquest::client::ClientOptions) -> Self {
        Self {
            options: Arc::new(options),
            slot: Arc::new(Mutex::new(None)),
        }
    }

    fn get_or_create_handle(&self) -> EasyHandleGuard<&Arc<Mutex<Option<MultiEasy>>>> {
        let handle = match {
            let mut slot = self.slot.lock().unwrap();
            slot.take()
        } {
            Some(handle) => handle,
            None => MultiEasy::new(),
        };
        EasyHandleGuard {
            slot: &self.slot,
            handle: ManuallyDrop::new(Mutex::new(handle)),
        }
    }
}

impl io::Read for CurlResponse {
    fn read(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
        todo!()
    }
}

impl nyquest::blocking::backend::BlockingResponse for CurlResponse {
    fn status(&self) -> u16 {
        self.status
    }

    fn content_length(&self) -> Option<u64> {
        self.content_length
    }

    fn get_header(&self, header: &str) -> nyquest::Result<Vec<String>> {
        Ok(self
            .headers
            .iter()
            .filter(|(k, _)| k.eq_ignore_ascii_case(header))
            .map(|(_, v)| v.clone())
            .collect())
    }

    fn text(&mut self) -> nyquest::Result<String> {
        let buf = self.bytes()?;
        #[cfg(feature = "charset")]
        if let Some((_, charset)) = self
            .get_header("content-type")?
            .pop()
            .unwrap_or_default()
            .split(';')
            .filter_map(|s| s.split_once('='))
            .find(|(k, _)| k.trim().eq_ignore_ascii_case("charset"))
        {
            if let Ok(decoded) = iconv_native::decode_lossy(&buf, charset.trim()) {
                return Ok(decoded);
            }
        }
        Ok(String::from_utf8_lossy(&buf).into_owned())
    }

    fn bytes(&mut self) -> nyquest::Result<Vec<u8>> {
        // TODO: proper timeouts
        self.handle
            .with_handle(|handle| handle.poll_until_whole_response(Duration::from_secs(30)))?;
        let buf = self
            .handle
            .with_handle(|handle| handle.take_response_buffer());
        Ok(buf)
    }
}

impl nyquest::blocking::backend::BlockingClient for CurlEasyClient {
    type Response = CurlResponse;

    fn request(&self, req: Request) -> nyquest::Result<Self::Response> {
        let mut handle = self.get_or_create_handle();
        // FIXME: properly concat base_url and url
        let url = concat_url(self.options.base_url.as_deref(), &req.relative_uri);
        handle.with_handle(|handle| handle.populate_request(&url, req, &self.options))?;
        // TODO: proper timeouts
        handle.with_handle(|handle| handle.poll_until_response_headers(Duration::from_secs(30)))?;
        let (status, content_length) = handle.with_handle(|handle| {
            Ok::<_, NyquestError>((handle.status()?, handle.content_length()?))
        })?;
        let mut headers_buf = handle.with_handle(|handle| handle.take_response_headers_buffer());
        let headers = headers_buf
            .iter_mut()
            .filter_map(|line| std::str::from_utf8_mut(&mut *line).ok())
            .filter_map(|line| line.split_once(':'))
            .map(|(k, v)| (k.into(), v.trim_start().into()))
            .collect();
        Ok(CurlResponse {
            status,
            content_length,
            headers,
            handle: handle.into_owned(),
        })
    }
}

impl nyquest::blocking::backend::BlockingBackend for crate::CurlBackend {
    type BlockingClient = CurlEasyClient;

    fn create_blocking_client(
        &self,
        options: nyquest::client::ClientOptions,
    ) -> nyquest::client::BuildClientResult<Self::BlockingClient> {
        Ok(CurlEasyClient::new(options))
    }
}
