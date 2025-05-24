use std::{
    os::raw::c_void,
    sync::{Arc, Mutex, MutexGuard},
};

use curl::{
    easy::{Easy, Easy2, Handler},
    ShareError,
};
use curl_sys::{
    curl_easy_setopt, curl_lock_access, curl_lock_data, curl_lock_function, curl_unlock_function,
    CURL, CURLE_OK, CURLOPT_SHARE, CURLSHE_OK, CURLSHOPT_LOCKFUNC, CURLSHOPT_SHARE,
    CURLSHOPT_UNLOCKFUNC, CURLSHOPT_USERDATA, CURL_LOCK_DATA_CONNECT, CURL_LOCK_DATA_COOKIE,
    CURL_LOCK_DATA_DNS, CURL_LOCK_DATA_SSL_SESSION,
};
use nyquest_interface::Result as NyquestResult;

use crate::error::IntoNyquestResult;

type MutexGuardStore = Mutex<Option<MutexGuard<'static, ()>>>; // One per curl share data type
struct RawShare {
    raw: *mut curl_sys::CURLSH,
    mutexes: [(Mutex<()>, MutexGuardStore); 7], // One per curl share data type
}

#[derive(Clone)]
pub(super) struct Share {
    raw: Arc<RawShare>,
}

pub(super) struct ShareHandle {
    _share: Arc<RawShare>,
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
            panic!(
                "blocking: Mutex {} is not unlocked before dropping share",
                unlocked_idx
            );
        }
        unsafe { curl_sys::curl_share_cleanup(self.raw) };
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

    pub fn get_handle(&self) -> ShareHandle {
        ShareHandle {
            _share: self.raw.clone(),
        }
    }

    /// # Safety
    ///
    /// Callers ensure that the ShareHandle should not be dropped earlier than the easy handle.
    pub unsafe fn bind_easy(&self, easy: &mut Easy) -> NyquestResult<()> {
        unsafe {
            let res = curl_easy_setopt(easy.raw(), CURLOPT_SHARE, self.raw.raw);
            if res != CURLE_OK {
                let err = curl::Error::new(res);
                return Err(err).into_nyquest_result("blocking bind_easy");
            }
        }
        Ok(())
    }

    /// # Safety
    ///
    /// Callers ensure that the ShareHandle should not be dropped earlier than the easy handle.
    pub unsafe fn bind_easy2<H: Handler>(&self, easy: &mut Easy2<H>) -> NyquestResult<()> {
        unsafe {
            let res = curl_easy_setopt(easy.raw(), CURLOPT_SHARE, self.raw.raw);
            if res != CURLE_OK {
                let err = curl::Error::new(res);
                return Err(err).into_nyquest_result("blocking bind_easy2");
            }
        }
        Ok(())
    }
}

impl RawShare {
    pub fn new() -> Self {
        let raw = unsafe { curl_sys::curl_share_init() };
        if raw.is_null() {
            panic!("blocking init failed alloc share");
        }
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

        unsafe {
            set_share_option(self.raw, CURLSHOPT_SHARE, CURL_LOCK_DATA_COOKIE).ok();
            set_share_option(self.raw, CURLSHOPT_SHARE, CURL_LOCK_DATA_DNS).ok();
            set_share_option(self.raw, CURLSHOPT_SHARE, CURL_LOCK_DATA_SSL_SESSION).ok();
            set_share_option(self.raw, CURLSHOPT_SHARE, CURL_LOCK_DATA_CONNECT).ok();
            set_share_option(self.raw, CURLSHOPT_SHARE, CURL_LOCK_DATA_PSL).ok();
            set_share_option(self.raw, CURLSHOPT_SHARE, CURL_LOCK_DATA_HSTS).ok();
        };
    }

    unsafe fn set_self_ptr(&self, ptr: *const Self) -> Result<(), ShareError> {
        let result = curl_sys::curl_share_setopt(self.raw, CURLSHOPT_USERDATA, ptr as *const _);
        if result != CURLSHE_OK {
            return Err(ShareError::new(result));
        }
        let result = curl_sys::curl_share_setopt(
            self.raw,
            CURLSHOPT_LOCKFUNC,
            lock_function as curl_lock_function,
        );
        if result != CURLSHE_OK {
            return Err(ShareError::new(result));
        }
        let result = curl_sys::curl_share_setopt(
            self.raw,
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
