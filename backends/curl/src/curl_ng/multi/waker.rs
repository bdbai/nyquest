use std::sync::{Arc, Weak};

use crate::curl_ng::multi::raw::RawMulti;

#[derive(Clone)]
pub struct MultiWaker<M> {
    multi: Weak<M>,
}

unsafe impl<M> Send for MultiWaker<M> {}
unsafe impl<M> Sync for MultiWaker<M> {}

impl<M> MultiWaker<M> {
    pub(super) fn new(raw: &Arc<M>) -> Self {
        MultiWaker {
            multi: Arc::downgrade(raw),
        }
    }
}

impl<M: AsRef<RawMulti>> MultiWaker<M> {
    pub fn wake(&self) {
        let Some(multi) = self.multi.upgrade() else {
            return;
        };
        let raw = (*multi).as_ref().raw();
        unsafe {
            curl_sys::curl_multi_wakeup(raw);
        }
    }
}
