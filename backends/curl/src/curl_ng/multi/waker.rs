use std::{
    ptr::NonNull,
    sync::{Arc, Mutex, Weak},
};

use curl_sys::CURLM_BAD_HANDLE;

use crate::curl_ng::{
    error_context::CurlMultiCodeContext,
    multi::{raw::RawMulti, IsSendWithMultiSet, IsSyncWithMultiSet},
    WithCurlCodeContext,
};

struct WakerCore {
    multi: Mutex<Option<NonNull<curl_sys::CURLM>>>,
}

#[derive(Clone)]
pub struct MultiWaker {
    core: Weak<WakerCore>,
}

pub struct WakeableMulti<M> {
    multi: M,
    waker_core: Arc<WakerCore>,
}

unsafe impl Send for WakerCore {}
unsafe impl Sync for WakerCore {}
unsafe impl<M: IsSendWithMultiSet> IsSendWithMultiSet for WakeableMulti<M> {}
unsafe impl<M: IsSyncWithMultiSet> IsSyncWithMultiSet for WakeableMulti<M> {}

impl WakerCore {
    fn wakeup(core: &Weak<Self>) -> Result<(), CurlMultiCodeContext> {
        let Some(this) = core.upgrade() else {
            return CURLM_BAD_HANDLE.with_multi_context("curl_multi_wakeup");
        };
        let guard = this.multi.lock().unwrap();
        let Some(raw) = guard.as_ref() else {
            return CURLM_BAD_HANDLE.with_multi_context("curl_multi_wakeup");
        };
        unsafe {
            // Make sure to call curl_multi_wakeup only when the mutex is held,
            // ensuring the multi handle is not concurrently cleaned up.
            curl_sys::curl_multi_wakeup(raw.as_ptr()).with_multi_context("curl_multi_wakeup")?;
        }
        Ok(())
    }

    fn invalidate(&self) {
        let mut guard = self.multi.lock().unwrap();
        *guard = None;
    }
}

impl MultiWaker {
    pub fn wakeup(&self) -> Result<(), CurlMultiCodeContext> {
        WakerCore::wakeup(&self.core)
    }
}

impl<M: AsRef<RawMulti>> WakeableMulti<M> {
    pub fn new(multi: M) -> Self {
        let waker_core = Arc::new(WakerCore {
            multi: Mutex::new(Some(multi.as_ref().raw)),
        });
        WakeableMulti { multi, waker_core }
    }
}

impl<M> WakeableMulti<M> {
    pub fn get_waker(&self) -> MultiWaker {
        MultiWaker {
            core: Arc::downgrade(&self.waker_core),
        }
    }
}

impl<M: AsRef<RawMulti>> AsRef<RawMulti> for WakeableMulti<M> {
    fn as_ref(&self) -> &RawMulti {
        self.multi.as_ref()
    }
}

impl<M> Drop for WakeableMulti<M> {
    fn drop(&mut self) {
        self.waker_core.invalidate();
    }
}
