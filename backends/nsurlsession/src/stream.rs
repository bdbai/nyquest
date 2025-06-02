#![allow(non_snake_case)]

use std::ffi::c_void;

use objc2::rc::Retained;
use objc2::{define_class, msg_send, AllocAnyThread};
use objc2_foundation::{NSError, NSInputStream, NSObjectProtocol, NSStreamStatus};

pub(crate) struct InputStreamIvars {}

define_class!(
    // SAFETY:
    // - The superclass NSObject does not have any subclassing requirements.
    // - `Delegate` does not implement `Drop`.
    #[unsafe(super = NSInputStream)]
    // #[thread_kind = MainThreadOnly]
    #[name = "Nyquest_InputStream"]
    #[ivars = InputStreamIvars]
    pub(crate) struct InputStream;

    // SAFETY: `NSObjectProtocol` has no safety requirements.
    unsafe impl NSObjectProtocol for InputStream {}

    impl InputStream {
        #[unsafe(method(open))]
        fn __open(&self) {
            todo!()
        }

        #[unsafe(method(close))]
        fn __close(&self) {
            todo!()
        }

        #[unsafe(method(read:maxLength:))]
        fn __read(&self, buffer: *mut u8, max_len: usize) -> isize {
            todo!()
        }

        #[unsafe(method(hasBytesAvailable))]
        fn __has_bytes_available(&self) -> bool {
            todo!()
        }

        #[unsafe(method(streamStatus))]
        fn __stream_status(&self) -> NSStreamStatus {
            todo!()
        }

        #[unsafe(method(streamError))]
        fn __stream_error(&self) -> *mut NSError {
            todo!()
        }

        // Private methods required by NSInputStream
        #[unsafe(method(_scheduleInCFRunLoop:forMode:))]
        fn __schedule_in_cf_runloop(&self, _runloop: *const c_void, _mode: *const c_void) {
            todo!()
        }
        #[unsafe(method(_unscheduleFromCFRunLoop:forMode:))]
        fn __unschedule_from_cf_runloop(&self, _runloop: *const c_void, _mode: *const c_void) {
            todo!()
        }
        #[unsafe(method(_setCFClientFlags:callback:context:))]
        fn __set_cf_client_flags(
            &self,
            _flags: u32,
            _callback: *const c_void,
            _context: *const c_void,
        ) -> bool {
            todo!()
        }
    }
);

impl InputStream {
    pub(crate) fn new() -> Retained<Self> {
        let this = Self::alloc().set_ivars(InputStreamIvars {});

        unsafe { msg_send![super(this), init] }
    }
}
