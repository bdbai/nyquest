#![allow(non_snake_case)]

use arc_swap::ArcSwapAny;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{define_class, msg_send, AllocAnyThread, DefinedClass, Message as _};
use objc2_foundation::{
    NSError, NSInputStream, NSObjectProtocol, NSRunLoop, NSRunLoopMode, NSStreamDelegate,
    NSStreamPropertyKey, NSStreamStatus,
};

use crate::retained_ext::SwappableRetained;

pub(crate) struct InputStreamIvars {
    delegate: ArcSwapAny<Option<SwappableRetained<ProtocolObject<dyn NSStreamDelegate>>>>,
    run_loop: ArcSwapAny<Option<SwappableRetained<NSRunLoop>>>,
    run_loop_mode: ArcSwapAny<Option<SwappableRetained<NSRunLoopMode>>>,
}

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
        fn __open(&self) { }

        #[unsafe(method(close))]
        fn __close(&self) {
            todo!("close")
        }

        #[unsafe(method(setDelegate:))]
        fn setDelegate(&self, delegate: Option<&ProtocolObject<dyn NSStreamDelegate>>) {
            self.callback_setDelegate(delegate);
        }

        #[unsafe(method(propertyForKey:))]
        fn propertyForKey(
            &self,
            key: &NSStreamPropertyKey,
        ) -> *mut AnyObject {
            todo!("propertyForKey")
        }

        #[unsafe(method(setProperty:forKey:))]
        fn setProperty_forKey(
            &self,
            property: Option<&AnyObject>,
            key: &NSStreamPropertyKey,
        ) -> bool {
            todo!("setProperty_forKey")
        }

        #[unsafe(method(scheduleInRunLoop:forMode:))]
        fn scheduleInRunLoop_forMode(
            &self,
            a_run_loop: &NSRunLoop,
            mode: &NSRunLoopMode,
        ) {
            self.callback_scheduleInRunLoop_forMode(a_run_loop, mode);
        }

        #[unsafe(method(removeFromRunLoop:forMode:))]
        fn removeFromRunLoop_forMode(
            &self,
            a_run_loop: &NSRunLoop,
            mode: &NSRunLoopMode,
        ) {
            todo!("removeFromRunLoop_forMode")
        }

        #[unsafe(method(streamStatus))]
        fn streamStatus(&self) -> NSStreamStatus {
            self.callback_streamStatus()
        }

        #[unsafe(method(streamError))]
        fn streamError(&self) -> *mut NSError {
            todo!("streamError")
        }

        #[unsafe(method(read:maxLength:))]
        fn __read(&self, buffer: *mut u8, max_len: usize) -> isize {
            todo!("read")
        }

        #[unsafe(method(hasBytesAvailable))]
        fn hasBytesAvailable(&self) -> bool {
            todo!("hasBytesAvailable")
        }
    }
);

impl InputStream {
    pub(crate) fn new() -> Retained<Self> {
        let this = Self::alloc().set_ivars(InputStreamIvars {
            delegate: ArcSwapAny::new(None),
            run_loop: ArcSwapAny::new(None),
            run_loop_mode: ArcSwapAny::new(None),
        });

        unsafe { msg_send![super(this), init] }
    }

    fn callback_setDelegate(&self, delegate: Option<&ProtocolObject<dyn NSStreamDelegate>>) {
        let delegate = delegate.map(|d| d.retain().into());
        self.ivars().delegate.store(delegate);
    }
    fn callback_scheduleInRunLoop_forMode(&self, a_run_loop: &NSRunLoop, mode: &NSRunLoopMode) {
        let ivars = self.ivars();
        ivars.run_loop.store(Some(a_run_loop.retain().into()));
        ivars.run_loop_mode.store(Some(mode.retain().into()));
    }
    fn callback_streamStatus(&self) -> NSStreamStatus {
        NSStreamStatus::NotOpen
    }
}
