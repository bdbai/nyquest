use std::io;
use std::mem::ManuallyDrop;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::multi_easy::MultiEasy;
use crate::url::concat_url;

pub struct CurlEasyBackend;

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
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        todo!()
    }
}

impl nyquest::blocking::backend::BlockingResponse for CurlResponse {
    fn status(&self) -> u16 {
        self.status
    }

    fn content_length(&self) -> Option<u64> {
        todo!()
    }

    fn get_header(&self, _header: &str) -> nyquest::Result<Vec<String>> {
        todo!()
    }

    fn text(&mut self) -> nyquest::Result<String> {
        // TODO: proper timeouts
        self.handle
            .with_handle(|handle| handle.poll_until_whole_response(Duration::from_secs(30)))?;
        let buf = self
            .handle
            .with_handle(|handle| handle.take_response_buffer());
        Ok(String::from_utf8_lossy(&buf).into_owned())
    }

    fn bytes(&mut self) -> nyquest::Result<Vec<u8>> {
        todo!()
    }
}

impl nyquest::blocking::backend::BlockingClient for CurlEasyClient {
    type Response = CurlResponse;

    fn request(
        &self,
        req: nyquest::Request<nyquest::blocking::Body>,
    ) -> nyquest::Result<Self::Response> {
        let mut handle = self.get_or_create_handle();
        // FIXME: properly concat base_url and url
        let url = concat_url(self.options.base_url.as_deref(), &req.relative_uri);
        handle.with_handle(|handle| handle.populate_request(&url, req, &self.options))?;
        // TODO: proper timeouts
        handle.with_handle(|handle| handle.poll_until_response_headers(Duration::from_secs(30)))?;
        let status = handle.with_handle(|handle| handle.status())?;
        Ok(CurlResponse {
            status,
            handle: handle.into_owned(),
        })
    }
}

impl nyquest::blocking::backend::BlockingBackend for CurlEasyBackend {
    type BlockingClient = CurlEasyClient;

    fn create_blocking_client(
        &self,
        options: nyquest::client::ClientOptions,
    ) -> nyquest::client::BuildClientResult<Self::BlockingClient> {
        Ok(CurlEasyClient::new(options))
    }
}
