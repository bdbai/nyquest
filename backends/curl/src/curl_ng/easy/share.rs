use std::ffi::c_void;
use std::pin::Pin;
use std::ptr::NonNull;
use std::sync::{Arc, Mutex, MutexGuard};

use curl::ShareError;
use curl_sys::{
    curl_lock_access, curl_lock_data, curl_lock_function, curl_unlock_function, CURL,
    CURLOPT_SHARE, CURLSHE_OK, CURLSHOPT_LOCKFUNC, CURLSHOPT_SHARE, CURLSHOPT_UNLOCKFUNC,
    CURLSHOPT_USERDATA, CURL_LOCK_DATA_CONNECT, CURL_LOCK_DATA_COOKIE, CURL_LOCK_DATA_DNS,
    CURL_LOCK_DATA_SSL_SESSION,
};
use pin_project_lite::pin_project;

use crate::curl_ng::easy::{AsRawEasyMut, RawEasy};
use crate::curl_ng::error_context::{CurlCodeContext, WithCurlCodeContext};

type MutexGuardStore = Mutex<Option<MutexGuard<'static, ()>>>; // One per curl share data type
struct RawShare {
    raw: NonNull<curl_sys::CURLSH>,
    mutexes: [(Mutex<()>, MutexGuardStore); 7], // One per curl share data type
}

#[derive(Clone)]
pub struct Share {
    raw: Arc<RawShare>,
}

pin_project! {
    pub struct ShareHandle<E> {
        #[pin]
        easy: E,
        share: Arc<RawShare>,
    }
}

impl Drop for RawShare {
    fn drop(&mut self) {
        let first_unlocked_mutex = self
            .mutexes
            .iter()
            .enumerate()
            .find(|(_idx, (_, guard_mutex))| guard_mutex.lock().unwrap().is_some())
            .map(|(idx, _)| idx);
        if let Some(unlocked_idx) = first_unlocked_mutex {
            panic!("blocking: Mutex {unlocked_idx} is not unlocked before dropping share");
        }
        unsafe { curl_sys::curl_share_cleanup(self.raw.as_ptr()) };
    }
}

impl Share {
    pub fn new() -> Self {
        let raw = Arc::new(RawShare::new());
        unsafe {
            raw.set_self_ptr(Arc::as_ptr(&raw))
                .expect("blocking: failed to set self pointer for share");
        }
        Share { raw }
    }

    pub fn spawn_easy<E>(&self, easy: E) -> ShareHandle<E> {
        ShareHandle {
            easy,
            share: self.raw.clone(),
        }
    }
}

impl RawShare {
    fn new() -> Self {
        let raw = unsafe { curl_sys::curl_share_init() };
        let raw = NonNull::new(raw).expect("blocking init failed alloc share");
        let mut sh = RawShare {
            raw,
            mutexes: Default::default(),
        };
        sh.init_options();
        sh
    }

    fn init_options(&mut self) {
        pub const CURL_LOCK_DATA_PSL: curl_lock_data = 6;
        pub const CURL_LOCK_DATA_HSTS: curl_lock_data = 7;

        let raw = self.raw.as_ptr();
        unsafe {
            set_share_option(raw, CURLSHOPT_SHARE, CURL_LOCK_DATA_COOKIE).ok();
            set_share_option(raw, CURLSHOPT_SHARE, CURL_LOCK_DATA_DNS).ok();
            set_share_option(raw, CURLSHOPT_SHARE, CURL_LOCK_DATA_SSL_SESSION).ok();
            set_share_option(raw, CURLSHOPT_SHARE, CURL_LOCK_DATA_CONNECT).ok();
            set_share_option(raw, CURLSHOPT_SHARE, CURL_LOCK_DATA_PSL).ok();
            set_share_option(raw, CURLSHOPT_SHARE, CURL_LOCK_DATA_HSTS).ok();
        };
    }

    unsafe fn set_self_ptr(&self, ptr: *const Self) -> Result<(), ShareError> {
        let raw = self.raw.as_ptr();
        let result = curl_sys::curl_share_setopt(raw, CURLSHOPT_USERDATA, ptr as *const _);
        if result != CURLSHE_OK {
            return Err(ShareError::new(result));
        }
        let result = curl_sys::curl_share_setopt(
            raw,
            CURLSHOPT_LOCKFUNC,
            lock_function as curl_lock_function,
        );
        if result != CURLSHE_OK {
            return Err(ShareError::new(result));
        }
        let result = curl_sys::curl_share_setopt(
            raw,
            CURLSHOPT_UNLOCKFUNC,
            unlock_function as curl_unlock_function,
        );
        if result != CURLSHE_OK {
            return Err(ShareError::new(result));
        }
        Ok(())
    }
}

unsafe impl Send for RawShare {}
unsafe impl Sync for RawShare {}

impl<E> ShareHandle<E> {
    pub fn as_easy_mut(self: Pin<&mut Self>) -> Pin<&mut E> {
        self.project().easy
    }
}

impl<E: AsRawEasyMut> ShareHandle<E> {
    fn bind_share(mut self: Pin<&mut Self>) -> Result<(), CurlCodeContext> {
        let this = self.as_mut().project();
        let raw = this.easy.as_raw_easy_mut();
        unsafe {
            raw.setopt_ptr(CURLOPT_SHARE, this.share.raw.as_ptr() as _)
                .with_easy_context("setopt CURLOPT_SHARE")
        }
    }
}

impl<E: AsRawEasyMut> AsRawEasyMut for ShareHandle<E> {
    fn init(mut self: Pin<&mut Self>) -> Result<(), CurlCodeContext> {
        self.as_mut().project().easy.init()?;
        self.bind_share()
    }

    fn as_raw_easy_mut(self: Pin<&mut Self>) -> Pin<&mut RawEasy> {
        self.project().easy.as_raw_easy_mut()
    }

    fn reset(mut self: Pin<&mut Self>) -> Result<(), CurlCodeContext> {
        self.as_mut().project().easy.reset()?;
        self.bind_share()
    }
}

unsafe fn set_share_option(
    share: *mut curl_sys::CURLSH,
    option: curl_sys::CURLSHoption,
    value: curl_lock_data,
) -> Result<(), ShareError> {
    let result = unsafe { curl_sys::curl_share_setopt(share, option, value) };
    if result == CURLSHE_OK {
        Ok(())
    } else {
        Err(ShareError::new(result))
    }
}

extern "C" fn lock_function(
    _curl: *mut CURL,
    data: curl_lock_data,
    _access: curl_lock_access,
    user_ptr: *mut c_void,
) {
    unsafe {
        let share = &*(user_ptr as *const RawShare);
        let (main_mutex, guard_mutex) =
            &share.mutexes[(data as usize).min(share.mutexes.len() - 1)];
        let guard = main_mutex.lock().unwrap();
        match &mut *guard_mutex.lock().unwrap() {
            Some(_) => {
                panic!("blocking: lock function called while already locked");
            }
            guard_slot @ None => {
                *guard_slot = std::mem::transmute::<
                    Option<MutexGuard<'_, ()>>,
                    Option<MutexGuard<'static, ()>>,
                >(Some(guard));
            }
        }
    }
}

extern "C" fn unlock_function(_curl: *mut CURL, data: curl_lock_data, user_ptr: *mut c_void) {
    unsafe {
        let share = &*(user_ptr as *const RawShare);
        let (_main_mutex, guard_mutex) =
            &share.mutexes[(data as usize).min(share.mutexes.len() - 1)];
        drop(guard_mutex.lock().unwrap().take());
    }
}
