use objc2::{define_class, msg_send, rc::Retained, AllocAnyThread};
use objc2_foundation::{
    NSObject, NSObjectProtocol, NSURLSessionDataDelegate, NSURLSessionDelegate,
    NSURLSessionTaskDelegate,
};

#[derive(Default)]
pub(super) struct BlockingDataTaskIvars {}

define_class!(
    // SAFETY:
    // - The superclass NSObject does not have any subclassing requirements.
    // - `Delegate` does not implement `Drop`.
    #[unsafe(super = NSObject)]
    // #[thread_kind = MainThreadOnly]
    #[name = "BlockingDataTaskDelegate"]
    #[ivars = BlockingDataTaskIvars]
    pub(super) struct BlockingDataTaskDelegate;

    // SAFETY: `NSObjectProtocol` has no safety requirements.
    unsafe impl NSObjectProtocol for BlockingDataTaskDelegate {}

    // SAFETY: `NSApplicationDelegate` has no safety requirements.
    unsafe impl NSURLSessionDelegate for BlockingDataTaskDelegate {}

    // SAFETY: `NSApplicationDelegate` has no safety requirements.
    unsafe impl NSURLSessionTaskDelegate for BlockingDataTaskDelegate {}

    unsafe impl NSURLSessionDataDelegate for BlockingDataTaskDelegate {}
);

impl BlockingDataTaskDelegate {
    pub(super) fn new() -> Retained<Self> {
        let this = Self::alloc().set_ivars(BlockingDataTaskIvars {});
        // SAFETY: The signature of `NSObject`'s `init` method is correct.
        unsafe { msg_send![super(this), init] }
    }
}
