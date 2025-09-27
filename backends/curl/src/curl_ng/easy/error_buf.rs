use std::borrow::Cow;
use std::cell::UnsafeCell;
use std::ffi::c_char;
use std::marker::PhantomPinned;
use std::mem::MaybeUninit;
use std::pin::Pin;

use curl_sys::CURL_ERROR_SIZE;
use pin_project_lite::pin_project;

use crate::curl_ng::{
    easy::{AsRawEasyMut, RawEasy},
    error_context::{CurlCodeContext, CurlErrorContext},
};

pub struct ErrorBuf {
    pub(super) buf: UnsafeCell<[MaybeUninit<c_char>; CURL_ERROR_SIZE]>,
}

unsafe impl Send for ErrorBuf {}
unsafe impl Sync for ErrorBuf {}

pin_project! {
    pub struct OwnedEasyWithErrorBuf<E> {
        #[pin]
        pub(super) easy: E,
        pub(super) error_buf: ErrorBuf,
        __pinned: PhantomPinned,
    }
}

impl ErrorBuf {
    pub fn new() -> Self {
        let mut buf = [MaybeUninit::uninit(); CURL_ERROR_SIZE];
        buf[0].write(0);
        ErrorBuf {
            buf: UnsafeCell::new(buf),
        }
    }

    fn to_string(&self) -> Cow<'_, str> {
        let cstr = unsafe { std::ffi::CStr::from_ptr(self.buf.get() as _) };
        cstr.to_string_lossy()
    }
}

impl ErrorBuf {
    pub fn with_easy_attached<'s, E: AsRawEasyMut, T>(
        self: Pin<&'s mut Self>,
        mut easy: Pin<&mut E>,
        callback: impl FnOnce(Pin<&mut E>) -> Result<T, CurlCodeContext>,
    ) -> Result<T, CurlErrorContext<'s>> {
        unsafe {
            easy.as_mut()
                .as_raw_easy_mut()
                .attach_error_buf(self.buf.get() as *mut c_char)
        }
        .map_err(|e| CurlErrorContext {
            code: e.code,
            msg: "".into(),
            context: e.context,
        })?;
        struct ErrorBufAttachGuard<'e, E: AsRawEasyMut> {
            easy: Pin<&'e mut E>,
        }
        impl<'e, E: AsRawEasyMut> Drop for ErrorBufAttachGuard<'e, E> {
            fn drop(&mut self) {
                self.easy
                    .as_mut()
                    .as_raw_easy_mut()
                    .detach_error_buf()
                    .expect("detach error buf");
            }
        }
        let res = {
            let mut guard = ErrorBufAttachGuard { easy };
            callback(guard.easy.as_mut())
        };
        res.map_err(|e| CurlErrorContext {
            code: e.code,
            msg: self.get_mut().to_string(),
            context: e.context,
        })
    }
}

impl<E> OwnedEasyWithErrorBuf<E> {
    pub fn new(easy: E) -> Self {
        OwnedEasyWithErrorBuf {
            easy,
            error_buf: ErrorBuf::new(),
            __pinned: PhantomPinned,
        }
    }

    pub fn as_easy_mut(self: Pin<&mut Self>) -> Pin<&mut E> {
        self.project().easy
    }

    pub fn with_error_message<T>(
        mut self: Pin<&mut Self>,
        callback: impl FnOnce(Pin<&mut Self>) -> Result<T, CurlCodeContext>,
    ) -> Result<T, CurlErrorContext<'_>> {
        let res = callback(self.as_mut());
        res.map_err(|e| CurlErrorContext {
            code: e.code,
            msg: self.project().error_buf.to_string(),
            context: e.context,
        })
    }
}

impl<E: AsRawEasyMut> OwnedEasyWithErrorBuf<E> {
    fn attach(self: Pin<&mut Self>) -> Result<(), CurlCodeContext> {
        let this = self.project();
        unsafe {
            (*this.error_buf.buf.get())[0].write(0);
            this.easy
                .as_raw_easy_mut()
                .attach_error_buf(this.error_buf.buf.get() as _)
        }
    }
    fn drop_detach(self: Pin<&mut Self>) {
        self.project()
            .easy
            .as_raw_easy_mut()
            .detach_error_buf()
            .expect("detach owned error buf");
    }
}

impl<E: AsRawEasyMut> AsRawEasyMut for OwnedEasyWithErrorBuf<E> {
    fn init(mut self: Pin<&mut Self>) -> Result<(), CurlCodeContext> {
        self.as_mut().attach()?;
        self.project().easy.init()
    }

    fn as_raw_easy_mut(self: Pin<&mut Self>) -> Pin<&mut RawEasy> {
        self.project().easy.as_raw_easy_mut()
    }

    fn reset(mut self: Pin<&mut Self>) -> Result<(), CurlCodeContext> {
        let this = self.as_mut().project();
        this.easy.reset()?;
        self.as_mut().attach()
    }
}
