use std::{ops::Deref, pin::Pin};

use slab::Slab;

use crate::{
    curl_ng::multi::MultiEasySet,
    r#async::AsyncHandler,
    request::{BoxEasyHandle, EasyHandle},
};

type Handle = BoxEasyHandle<AsyncHandler>;

#[derive(Default)]
pub(super) struct SlabMultiSet {
    pub(super) slab: Slab<Handle>,
}

unsafe impl MultiEasySet for SlabMultiSet {
    type Ptr = Box<EasyHandle<AsyncHandler>>;

    fn add(&mut self, item: Pin<Self::Ptr>) -> usize {
        self.slab.insert(item)
    }

    fn remove(&mut self, token: usize) -> Option<Pin<Self::Ptr>> {
        self.slab.try_remove(token)
    }

    unsafe fn lookup(&mut self, token: usize) -> Option<Pin<&mut <Self::Ptr as Deref>::Target>> {
        self.slab.get_mut(token).map(|e| e.as_mut())
    }

    fn is_empty(&self) -> bool {
        self.slab.is_empty()
    }

    fn shrink_to_fit(&mut self) {
        self.slab.shrink_to_fit()
    }

    type IterMut<'s> = slab::IterMut<'s, Pin<Self::Ptr>>;
    fn iter_mut<'s>(&'s mut self) -> Self::IterMut<'s> {
        self.slab.iter_mut()
    }
}
