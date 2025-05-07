#![allow(non_snake_case)]

use block2::DynBlock;
use objc2::rc::Retained;
use objc2::{define_class, msg_send, AllocAnyThread, ClassType};
use objc2_foundation::{
    NSObject, NSObjectProtocol, NSURLAuthenticationChallenge, NSURLCredential, NSURLSession,
    NSURLSessionAuthChallengeDisposition, NSURLSessionDelegate,
};
use objc2_security::sec_trust;

define_class!(
    // SAFETY:
    // - The superclass NSObject does not have any subclassing requirements.
    // - `Delegate` does not implement `Drop`.
    #[unsafe(super = NSObject)]
    // #[thread_kind = MainThreadOnly]
    #[name = "Nyquest_BypassServerVerifyDelegate"]
    pub(crate) struct BypassServerVerifyDelegate;

    // SAFETY: `NSObjectProtocol` has no safety requirements.
    unsafe impl NSObjectProtocol for BypassServerVerifyDelegate {}

    // SAFETY: `NSApplicationDelegate` has no safety requirements.
    unsafe impl NSURLSessionDelegate for BypassServerVerifyDelegate {
        #[unsafe(method(URLSession:didReceiveChallenge:completionHandler:))]
        unsafe fn URLSession_didReceiveChallenge_completionHandler(
            &self,
            session: &NSURLSession,
            challenge: &NSURLAuthenticationChallenge,
            completion_handler: &DynBlock<
                dyn Fn(NSURLSessionAuthChallengeDisposition, *mut NSURLCredential),
            >,
        ) {
            self.callback_URLSession_didReceiveChallenge_completionHandler(
                session,
                challenge,
                completion_handler,
            );
        }
    }
);

impl BypassServerVerifyDelegate {
    pub(crate) fn new() -> Retained<Self> {
        let this = Self::alloc();
        // SAFETY: The signature of `NSObject`'s `init` method is correct.
        unsafe { msg_send![this, init] }
    }

    fn callback_URLSession_didReceiveChallenge_completionHandler(
        &self,
        _session: &NSURLSession,
        challenge: &NSURLAuthenticationChallenge,
        completion_handler: &DynBlock<
            dyn Fn(NSURLSessionAuthChallengeDisposition, *mut NSURLCredential),
        >,
    ) {
        let trust_ref: *mut sec_trust = unsafe {
            let protectionSpace = challenge.protectionSpace();
            msg_send![&protectionSpace, serverTrust]
        };
        let cred: Retained<NSURLCredential> = unsafe {
            msg_send![
                NSURLCredential::class(),
                credentialWithTrust: trust_ref
            ]
        };
        completion_handler.call((
            NSURLSessionAuthChallengeDisposition::UseCredential,
            &*cred as *const NSURLCredential as _,
        ));
    }
}
