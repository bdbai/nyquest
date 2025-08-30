use std::{
    ops::{Deref, DerefMut},
    pin::Pin,
};

use crate::curl_ng::{
    easy_ref::AsRawEasyMut,
    error_context::{CurlMultiCodeContext, WithCurlCodeContext},
};

use super::raw_multi::RawMulti;

/// # Safety
///
/// Implementor must not invalidate any easy handles (say dropping). This would
/// cause uaf because the easy handle might still be attached to the multi
/// handle.
pub unsafe trait MultiEasySet {
    type Token;
    type Ptr: DerefMut;

    fn add(&mut self, item: Pin<Self::Ptr>) -> Self::Token;
    fn remove(&mut self, token: Self::Token) -> Option<Pin<Self::Ptr>>;
    /// # Safety
    ///
    /// Caller must ensure that the underlying easy handle remains same and
    /// valid.
    unsafe fn lookup<'s>(
        &mut self,
        token: &Self::Token,
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

impl<M: AsMut<RawMulti>, S> AsMut<RawMulti> for MultiWithSet<M, S> {
    fn as_mut(&mut self) -> &mut RawMulti {
        self.multi.as_mut()
    }
}

impl<M, S> MultiWithSet<M, S> {
    pub fn new(multi: M, set: S) -> Self {
        MultiWithSet { multi, set }
    }
}

impl<M: AsMut<RawMulti>, S> MultiWithSet<M, S> {}

impl<M: AsMut<RawMulti>, S: MultiEasySet> MultiWithSet<M, S>
where
    <S::Ptr as Deref>::Target: AsRawEasyMut,
{
    pub fn add(&mut self, mut item: Pin<S::Ptr>) -> Result<S::Token, CurlMultiCodeContext> {
        let easy = item.as_mut().as_raw_easy_mut();
        let raw_multi = self.multi.as_mut();
        unsafe { curl_sys::curl_multi_add_handle(raw_multi.raw(), easy.raw()) }
            .with_multi_context("curl_multi_add_handle")?;
        Ok(self.set.add(item))
    }

    pub fn remove(&mut self, token: S::Token) -> Result<Option<Pin<S::Ptr>>, CurlMultiCodeContext> {
        let Some(mut item) = self.set.remove(token) else {
            return Ok(None);
        };
        let easy = item.as_mut().as_raw_easy_mut();
        let raw_multi = self.multi.as_mut();
        unsafe { curl_sys::curl_multi_remove_handle(raw_multi.raw(), easy.raw()) }
            .with_multi_context("curl_multi_remove_handle")?;
        Ok(Some(item))
    }

    /// # Safety
    ///
    /// Caller must ensure that the underlying easy handle remains same and
    /// valid.
    pub unsafe fn lookup<'s>(
        &mut self,
        token: &S::Token,
    ) -> Option<Pin<&'s mut <S::Ptr as Deref>::Target>> {
        unsafe { self.set.lookup(token) }
    }
}
