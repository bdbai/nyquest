#![allow(non_snake_case)]

use std::sync::atomic::{AtomicBool, Ordering};

use arc_swap::ArcSwapAny;
use block2::DynBlock;
use objc2::rc::Retained;
use objc2::{define_class, msg_send, AllocAnyThread, DefinedClass};
use objc2_foundation::{
    NSCopying, NSData, NSError, NSHTTPURLResponse, NSObject, NSObjectProtocol, NSURLResponse,
    NSURLSession, NSURLSessionDataDelegate, NSURLSessionDataTask, NSURLSessionDelegate,
    NSURLSessionResponseDisposition, NSURLSessionTask, NSURLSessionTaskDelegate,
};

use super::generic_waker::GenericWaker;
use super::ivars::{DataTaskIvars, DataTaskIvarsShared};

define_class!(
    // SAFETY:
    // - The superclass NSObject does not have any subclassing requirements.
    // - `Delegate` does not implement `Drop`.
    #[unsafe(super = NSObject)]
    // #[thread_kind = MainThreadOnly]
    #[name = "Nyquest_DataTaskDelegate"]
    #[ivars = DataTaskIvars]
    pub(crate) struct DataTaskDelegate;

    // SAFETY: `NSObjectProtocol` has no safety requirements.
    unsafe impl NSObjectProtocol for DataTaskDelegate {}

    // SAFETY: `NSApplicationDelegate` has no safety requirements.
    unsafe impl NSURLSessionDelegate for DataTaskDelegate {}

    // SAFETY: `NSApplicationDelegate` has no safety requirements.
    unsafe impl NSURLSessionTaskDelegate for DataTaskDelegate {
        #[unsafe(method(didCompleteWithError:))]
        fn URLSession_task_didCompleteWithError(
            &self,
            session: &NSURLSession,
            task: &NSURLSessionTask,
            error: Option<&NSError>,
        ) {
            self.callback_URLSession_task_didCompleteWithError(session, task, error);
        }
    }

    unsafe impl NSURLSessionDataDelegate for DataTaskDelegate {
        #[unsafe(method(didReceiveResponse:completionHandler:))]
        fn URLSession_dataTask_didReceiveResponse_completionHandler(
            &self,
            session: &NSURLSession,
            data_task: &NSURLSessionDataTask,
            response: &NSURLResponse,
            completion_handler: &DynBlock<dyn Fn(NSURLSessionResponseDisposition)>,
        ) {
            self.callback_URLSession_dataTask_didReceiveResponse_completionHandler(
                session,
                data_task,
                response,
                completion_handler,
            );
        }

        #[unsafe(method(dataTask:didReceiveData:))]
        fn URLSession_dataTask_didReceiveData(
            &self,
            session: &NSURLSession,
            data_task: &NSURLSessionDataTask,
            data: &NSData,
        ) {
            self.callback_URLSession_dataTask_didReceiveData(session, data_task, data);
        }
    }
);

pub(crate) struct DataTaskSharedContextRetained {
    retained: Retained<DataTaskDelegate>,
}

impl DataTaskDelegate {
    pub(crate) fn new(waker: GenericWaker) -> Retained<Self> {
        let this = Self::alloc().set_ivars(DataTaskIvars {
            // continue_response_block: ArcSwapAny::new(None),
            shared: DataTaskIvarsShared {
                response: ArcSwapAny::new(None),
                waker,
                completed: AtomicBool::new(false),
                client_error: ArcSwapAny::new(None),
                response_buffer: Default::default(),
            },
        });
        // SAFETY: The signature of `NSObject`'s `init` method is correct.
        unsafe { msg_send![super(this), init] }
    }

    pub(crate) fn into_shared(retained: Retained<Self>) -> DataTaskSharedContextRetained {
        DataTaskSharedContextRetained { retained }
    }

    fn callback_URLSession_dataTask_didReceiveResponse_completionHandler(
        &self,
        _session: &NSURLSession,
        data_task: &NSURLSessionDataTask,
        response: &NSURLResponse,
        completion_handler: &DynBlock<dyn Fn(NSURLSessionResponseDisposition)>,
    ) {
        unsafe {
            data_task.suspend();
        }
        completion_handler.call((NSURLSessionResponseDisposition::Allow,));
        let ivars = self.ivars();
        ivars.shared.response.store(Some(response.copy().into()));
        ivars.shared.waker.wake();
    }
    fn callback_URLSession_task_didCompleteWithError(
        &self,
        _session: &NSURLSession,
        _task: &NSURLSessionTask,
        error: Option<&NSError>,
    ) {
        self.ivars().shared.completed.store(true, Ordering::Release);
        if let Some(error) = error {
            self.ivars()
                .shared
                .client_error
                .store(Some(error.copy().into()));
            self.ivars().shared.waker.wake();
        }
    }
    fn callback_URLSession_dataTask_didReceiveData(
        &self,
        _session: &NSURLSession,
        _data_task: &NSURLSessionDataTask,
        data: &NSData,
    ) {
        let mut buffer = self.ivars().shared.response_buffer.lock().unwrap();
        unsafe {
            buffer.extend_from_slice(data.as_bytes_unchecked());
        }
    }
}

impl DataTaskSharedContextRetained {
    pub(crate) fn waker_ref(&self) -> &GenericWaker {
        &self.retained.ivars().shared.waker
    }

    pub(crate) fn try_take_response(
        &self,
    ) -> Result<Option<Retained<NSHTTPURLResponse>>, Retained<NSError>> {
        if let Some(error) = self.retained.ivars().shared.client_error.swap(None) {
            return Err(error.into());
        }
        let response = self.retained.ivars().shared.response.swap(None);
        Ok(response.and_then(|res| Some(res.0.downcast::<NSHTTPURLResponse>().ok()?.into())))
    }

    pub(crate) fn is_completed(&self) -> bool {
        self.retained
            .ivars()
            .shared
            .completed
            .load(Ordering::Acquire)
    }

    pub(crate) fn take_response_buffer(&self) -> Vec<u8> {
        let mut buffer = self.retained.ivars().shared.response_buffer.lock().unwrap();
        std::mem::take(&mut *buffer)
    }
}

// Safety:
// `IvarsShared` may be dropped when any of the retained objects are dropped, hence Send is required.
// `IvarsShared` may be shared by sending retained objects to other threads, hence Sync is required.
unsafe impl Send for DataTaskSharedContextRetained where DataTaskIvarsShared: Send + Sync {}
// Safety:
// `IvarsShared` may be dropped when any thread holding a reference to the retained object drops it, hence Send is required.
// `IvarsShared` may be shared by sharing a retained object among threads, hence Sync is required.
unsafe impl Sync for DataTaskSharedContextRetained where DataTaskIvarsShared: Send + Sync {}
