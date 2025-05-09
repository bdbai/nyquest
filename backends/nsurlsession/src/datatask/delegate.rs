#![allow(non_snake_case)]

use std::ptr::null_mut;
use std::sync::atomic::{AtomicBool, Ordering};

use arc_swap::ArcSwapAny;
use block2::DynBlock;
use nyquest_interface::{Error as NyquestError, Result as NyquestResult};
use objc2::rc::Retained;
use objc2::{define_class, msg_send, AllocAnyThread, DefinedClass};
use objc2_foundation::{
    NSCopying, NSData, NSError, NSHTTPURLResponse, NSObject, NSObjectProtocol, NSURLRequest,
    NSURLResponse, NSURLSession, NSURLSessionDataDelegate, NSURLSessionDataTask,
    NSURLSessionDelegate, NSURLSessionResponseDisposition, NSURLSessionTask,
    NSURLSessionTaskDelegate,
};

use crate::error::IntoNyquestResult;

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
        #[unsafe(method(URLSession:task:willPerformHTTPRedirection:newRequest:completionHandler:))]
        fn URLSession_task_willPerformHTTPRedirection_newRequest_completionHandler(
            &self,
            session: &NSURLSession,
            task: &NSURLSessionTask,
            response: &NSHTTPURLResponse,
            request: &NSURLRequest,
            completion_handler: &DynBlock<dyn Fn(*mut NSURLRequest)>,
        ) {
            self.callback_URLSession_task_willPerformHTTPRedirection_newRequest_completionHandler(
                session,
                task,
                response,
                request,
                completion_handler,
            );
        }
        #[unsafe(method(URLSession:task:didCompleteWithError:))]
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
        #[unsafe(method(URLSession:dataTask:didReceiveResponse:completionHandler:))]
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

        #[unsafe(method(URLSession:dataTask:didReceiveData:))]
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
    pub(crate) fn new(waker: GenericWaker, allow_redirects: bool) -> Retained<Self> {
        let this = Self::alloc().set_ivars(DataTaskIvars {
            // continue_response_block: ArcSwapAny::new(None),
            shared: DataTaskIvarsShared {
                response: ArcSwapAny::new(None),
                waker,
                completed: AtomicBool::new(false),
                received_error: Default::default(),
                response_buffer: Default::default(),
                max_response_buffer_size: u64::MAX.into(),
            },
            redirects_allowed: if allow_redirects { 30 } else { 0 }.into(),
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
    fn callback_URLSession_task_willPerformHTTPRedirection_newRequest_completionHandler(
        &self,
        _session: &NSURLSession,
        _task: &NSURLSessionTask,
        _response: &NSHTTPURLResponse,
        request: &NSURLRequest,
        completion_handler: &DynBlock<dyn Fn(*mut NSURLRequest)>,
    ) {
        let ivars = &self.ivars().redirects_allowed;
        let allowed = ivars
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_sub(1))
            .is_ok();
        if allowed {
            completion_handler.call((request as *const _ as _,));
        } else {
            completion_handler.call((null_mut(),));
        }
    }
    fn callback_URLSession_task_didCompleteWithError(
        &self,
        _session: &NSURLSession,
        _task: &NSURLSessionTask,
        error: Option<&NSError>,
    ) {
        let ivars = self.ivars();
        ivars.shared.completed.store(dbg!(true), Ordering::SeqCst);
        if let Some(error) = error {
            ivars.set_error(error.copy());
        }
        ivars.shared.waker.wake();
    }
    fn callback_URLSession_dataTask_didReceiveData(
        &self,
        _session: &NSURLSession,
        data_task: &NSURLSessionDataTask,
        data: &NSData,
    ) {
        let ivars = self.ivars();
        let mut buffer = ivars.shared.response_buffer.lock().unwrap();
        let data = unsafe { data.as_bytes_unchecked() };
        if dbg!(buffer.len()) + dbg!(data.len())
            > dbg!(ivars
                .shared
                .max_response_buffer_size
                .load(Ordering::Acquire) as usize)
        {
            drop(buffer);
            ivars.set_error(NyquestError::ResponseTooLarge);
            unsafe {
                data_task.cancel();
            }
            return;
        }
        buffer.extend_from_slice(data);
    }
}

impl DataTaskSharedContextRetained {
    pub(crate) fn waker_ref(&self) -> &GenericWaker {
        &self.retained.ivars().shared.waker
    }

    pub(crate) fn try_take_response(&self) -> NyquestResult<Option<Retained<NSHTTPURLResponse>>> {
        let shared = &self.retained.ivars().shared;
        if let Some(error) = shared.received_error.lock().unwrap().take() {
            return Err(error);
        }
        let response = shared.response.swap(None);
        Ok(response.and_then(|res| res.0.downcast::<NSHTTPURLResponse>().ok()))
    }

    pub(crate) fn is_completed(&self) -> bool {
        self.retained
            .ivars()
            .shared
            .completed
            .load(Ordering::SeqCst)
    }

    pub(crate) fn take_response_buffer(&self) -> NyquestResult<Vec<u8>> {
        self.with_response_buffer_for_stream_mut(std::mem::take)
    }

    pub(crate) fn with_response_buffer_for_stream_mut<T>(
        &self,
        f: impl FnOnce(&mut Vec<u8>) -> T,
    ) -> NyquestResult<T> {
        let shared = &self.retained.ivars().shared;

        let err = shared.received_error.lock().unwrap().take();
        err.map(Err::<(), _>).transpose().into_nyquest_result()?;

        let mut buffer = shared.response_buffer.lock().unwrap();
        if buffer.len() > shared.max_response_buffer_size.load(Ordering::Acquire) as usize {
            return Err(NyquestError::ResponseTooLarge);
        }
        Ok(f(&mut buffer))
    }

    pub(crate) fn set_max_response_buffer_size(&self, size: u64) {
        self.retained
            .ivars()
            .shared
            .max_response_buffer_size
            .store(size, Ordering::Release);
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
