use std::sync::{
    atomic::{AtomicPtr, Ordering},
    Arc,
};

use crate::curl_ng::multi_set::{IsSendWithMultiSet, IsSyncWithMultiSet};

use super::raw_multi::RawMulti;

#[derive(Clone)]
pub struct MultiWaker {
    multi: Arc<AtomicPtr<curl_sys::CURLM>>,
}

pub struct WakerRegisteredMulti<M> {
    waker: MultiWaker,
    multi: M,
}

unsafe impl<M: IsSendWithMultiSet> IsSendWithMultiSet for WakerRegisteredMulti<M> where
    MultiWaker: Send
{
}
unsafe impl<M: IsSyncWithMultiSet> IsSyncWithMultiSet for WakerRegisteredMulti<M> where
    MultiWaker: Sync
{
}

impl MultiWaker {
    pub fn new() -> Self {
        MultiWaker {
            multi: Arc::new(AtomicPtr::new(std::ptr::null_mut())),
        }
    }

    pub fn register<M: AsMut<RawMulti>>(self, mut multi: M) -> WakerRegisteredMulti<M> {
        let raw_multi_ptr = multi.as_mut().raw();
        self.multi.store(raw_multi_ptr, Ordering::Release);
        WakerRegisteredMulti { waker: self, multi }
    }

    pub fn wake(&self) -> Result<(), curl::MultiError> {
        // Safety: When the registered multi is dropped, it will set the
        // pointer to null. Otherwise, the pointer is guaranteed to be valid.
        let multi_ptr = self.multi.load(Ordering::Acquire);
        if !multi_ptr.is_null() {
            let code = unsafe { curl_sys::curl_multi_wakeup(multi_ptr) };
            if code != curl_sys::CURLM_OK {
                return Err(curl::MultiError::new(code));
            }
        }
        Ok(())
    }
}

impl<M: AsMut<RawMulti>> AsMut<RawMulti> for WakerRegisteredMulti<M> {
    fn as_mut(&mut self) -> &mut RawMulti {
        self.multi.as_mut()
    }
}

impl<M> Drop for WakerRegisteredMulti<M> {
    fn drop(&mut self) {
        self.waker
            .multi
            .store(std::ptr::null_mut(), Ordering::Release);
    }
}
