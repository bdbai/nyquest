use std::sync::atomic::AtomicBool;
use std::sync::Mutex;

use arc_swap::ArcSwapAny;
use objc2_foundation::{NSError, NSURLResponse};

use super::generic_waker::GenericWaker;
use super::retained_ext::SwappableRetained;

pub(crate) struct DataTaskIvars {
    // pub(super) continue_response_block:
    //     ArcSwapAny<Option<SwappableRcBlock<dyn Fn(NSURLSessionResponseDisposition)>>>,
    pub(super) shared: DataTaskIvarsShared,
}

pub(super) struct DataTaskIvarsShared {
    pub(super) response: ArcSwapAny<Option<SwappableRetained<NSURLResponse>>>,
    pub(super) waker: GenericWaker,
    pub(super) completed: AtomicBool,
    pub(super) client_error: ArcSwapAny<Option<SwappableRetained<NSError>>>,
    pub(super) response_buffer: Mutex<Vec<u8>>,
}
