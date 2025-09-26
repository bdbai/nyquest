use std::{
    ops::{Deref, DerefMut},
    pin::Pin,
    ptr::null_mut,
    sync::Arc,
};

use crate::curl_ng::{
    easy::AsRawEasyMut,
    error_context::{CurlMultiCodeContext, WithCurlCodeContext},
    multi::raw::RawMulti,
    CurlCodeContext, CurlErrorContext,
};

/// # Safety
///
/// Implementor must not invalidate any easy handles (say dropping). This would
/// cause uaf because the easy handle might still be attached to the multi
/// handle.
pub unsafe trait MultiEasySet {
    type Ptr: DerefMut;

    fn add(&mut self, item: Pin<Self::Ptr>) -> usize;
    fn remove(&mut self, token: usize) -> Option<Pin<Self::Ptr>>;
    /// # Safety
    ///
    /// Caller must ensure that the underlying easy handle remains same and
    /// valid.
    unsafe fn lookup<'s>(
        &mut self,
        token: usize,
    ) -> Option<Pin<&'s mut <Self::Ptr as Deref>::Target>>;
}

pub unsafe trait IsSendWithMultiSet {}
pub unsafe trait IsSyncWithMultiSet {}

pub struct MultiWithSet<M, S> {
    multi: M,
    set: S,
}

unsafe impl<M: IsSendWithMultiSet, S: IsSendWithMultiSet> Send for MultiWithSet<M, S> {}
unsafe impl<M: IsSyncWithMultiSet, S: IsSyncWithMultiSet> Sync for MultiWithSet<M, S> {}

impl<M, S> MultiWithSet<M, S> {
    pub fn new(multi: M, set: S) -> Self {
        MultiWithSet { multi, set }
    }
}

impl<M, S> MultiWithSet<Arc<M>, S> {
    pub fn get_waker(&self) -> super::MultiWaker<M> {
        super::MultiWaker::new(&self.multi)
    }
}

impl<M: AsMut<RawMulti>, S: MultiEasySet> MultiWithSet<M, S>
where
    <S::Ptr as Deref>::Target: AsRawEasyMut,
{
    pub fn add(&mut self, mut item: Pin<S::Ptr>) -> Result<usize, CurlMultiCodeContext> {
        let easy = item.as_mut().as_raw_easy_mut();
        let raw_multi = self.multi.as_mut().raw();
        unsafe { curl_sys::curl_multi_add_handle(raw_multi, easy.raw()) }
            .with_multi_context("curl_multi_add_handle")?;
        let raw_easy = item.as_mut().as_raw_easy_mut().raw();
        let token = self.set.add(item);
        unsafe {
            curl_sys::curl_easy_setopt(raw_easy, curl_sys::CURLOPT_PRIVATE, token)
                .with_easy_context("multi add set private")
                .expect("Failed to set private option")
        };
        Ok(token)
    }

    pub fn remove(&mut self, token: usize) -> Result<Option<Pin<S::Ptr>>, CurlMultiCodeContext> {
        let Some(mut item) = self.set.remove(token) else {
            return Ok(None);
        };
        let easy = item.as_mut().as_raw_easy_mut();
        let raw_multi = self.multi.as_mut().raw();
        unsafe { curl_sys::curl_multi_remove_handle(raw_multi, easy.raw()) }
            .with_multi_context("curl_multi_remove_handle")?;
        Ok(Some(item))
    }

    pub fn perform(&mut self) -> Result<i32, CurlMultiCodeContext> {
        unsafe {
            let mut ret = 0;
            curl_sys::curl_multi_perform(self.multi.as_mut().raw(), &mut ret)
                .with_multi_context("curl_multi_perform")?;
            Ok(ret)
        }
    }

    pub fn lookup<'s>(&mut self, token: usize) -> Option<Pin<&'s mut <S::Ptr as Deref>::Target>> {
        unsafe { self.set.lookup(token) }
    }

    pub fn messages(
        &mut self,
        mut callback: impl FnMut(
            Pin<&mut <S::Ptr as Deref>::Target>,
            Option<Result<(), CurlCodeContext>>,
        ),
    ) -> Result<(), CurlMultiCodeContext> {
        unsafe {
            let mut queue = 0;
            loop {
                let msg_ptr = curl_sys::curl_multi_info_read(self.multi.as_mut().raw(), &mut queue);
                if msg_ptr.is_null() {
                    break;
                }
                let msg = &*msg_ptr;
                let done_result = if msg.msg == curl_sys::CURLMSG_DONE {
                    Some((msg.data as curl_sys::CURLcode).with_easy_context("curl_multi_info_read"))
                } else {
                    None
                };
                let token = curl_sys::curl_easy_getinfo(msg.easy_handle, curl_sys::CURLINFO_PRIVATE)
                    as usize;
                if let Some(mut easy) = self.lookup(token) {
                    if easy.as_mut().as_raw_easy_mut().raw() == msg.easy_handle {
                        callback(easy, done_result);
                    }
                }
            }
            Ok(())
        }
    }
}

impl<M: AsRef<RawMulti>, S> MultiWithSet<M, S> {
    pub fn poll(&self, timeout_ms: i32) -> Result<u32, CurlMultiCodeContext> {
        unsafe {
            let mut ret = 0;
            curl_sys::curl_multi_poll(
                self.multi.as_ref().raw(),
                null_mut(),
                0,
                timeout_ms,
                &mut ret,
            )
            .with_multi_context("curl_multi_poll")?;
            Ok(ret as u32)
        }
    }
}
