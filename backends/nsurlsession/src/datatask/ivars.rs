use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::Mutex;

use arc_swap::ArcSwapAny;
use nyquest_interface::Error as NyquestError;
use objc2_foundation::NSURLResponse;

use crate::error::IntoNyquestResult;

use super::generic_waker::GenericWaker;
use super::retained_ext::SwappableRetained;

pub(crate) struct DataTaskIvars {
    // pub(super) continue_response_block:
    //     ArcSwapAny<Option<SwappableRcBlock<dyn Fn(NSURLSessionResponseDisposition)>>>,
    pub(super) shared: DataTaskIvarsShared,
    pub(super) redirects_allowed: AtomicU8,
}

pub(super) struct DataTaskIvarsShared {
    pub(super) response: ArcSwapAny<Option<SwappableRetained<NSURLResponse>>>,
    pub(super) waker: GenericWaker,
    pub(super) completed: AtomicBool,
    pub(super) received_error: Mutex<Option<NyquestError>>,
    pub(super) max_response_buffer_size: AtomicU64,
    pub(super) response_buffer: Mutex<Vec<u8>>,
}

impl DataTaskIvars {
    pub(super) fn set_error<E>(&self, error: E)
    where
        Result<(), E>: IntoNyquestResult<()>,
    {
        self.shared.completed.store(true, Ordering::SeqCst);
        let error = Err(error).into_nyquest_result().unwrap_err();
        if let error_slot @ None = &mut *self.shared.received_error.lock().unwrap() {
            *error_slot = Some(error);
        }
        self.shared.waker.wake();
    }
}
