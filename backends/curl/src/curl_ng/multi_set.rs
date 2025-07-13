use super::{easy_ref::EasyWithErrorBufRef, raw_multi::RawMulti};

/// # Safety
///
/// Implementor must not invalidate any easy handles (say dropping). This would
/// cause uaf because the easy handle might still be attached to the multi
/// handle.
pub unsafe trait MultiEasySet {
    type Token;
    type Item;

    fn add(&mut self, item: Self::Item) -> Self::Token;
    fn remove(&mut self, token: Self::Token) -> Option<Self::Item>;
    /// # Safety
    ///
    /// Caller must ensure that the underlying easy handle remains same and
    /// valid.
    unsafe fn lookup<'s>(&mut self, token: &Self::Token) -> Option<&'s mut Self::Item>;
    fn map_easy_mut(item: &mut Self::Item) -> EasyWithErrorBufRef;
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

impl<M: AsMut<RawMulti>, S: MultiEasySet> MultiWithSet<M, S> {
    pub fn add(&mut self, mut item: S::Item) -> Result<S::Token, curl::MultiError> {
        let mut easy = S::map_easy_mut(&mut item);
        let raw_multi = self.multi.as_mut();
        let code = unsafe { curl_sys::curl_multi_add_handle(raw_multi.raw(), easy.raw()) };
        cvt(code)?;
        Ok(self.set.add(item))
    }

    pub fn remove(&mut self, token: S::Token) -> Result<Option<S::Item>, curl::MultiError> {
        let Some(mut item) = self.set.remove(token) else {
            return Ok(None);
        };
        let mut easy = S::map_easy_mut(&mut item);
        let raw_multi = self.multi.as_mut();
        let code = unsafe { curl_sys::curl_multi_remove_handle(raw_multi.raw(), easy.raw()) };
        cvt(code)?;
        Ok(Some(item))
    }

    /// # Safety
    ///
    /// Caller must ensure that the underlying easy handle remains same and
    /// valid.
    pub unsafe fn lookup<'s>(&mut self, token: &S::Token) -> Option<&'s mut S::Item> {
        unsafe { self.set.lookup(token) }
    }
}

fn cvt(code: curl_sys::CURLMcode) -> Result<(), curl::MultiError> {
    if code == curl_sys::CURLM_OK {
        Ok(())
    } else {
        Err(curl::MultiError::new(code))
    }
}
