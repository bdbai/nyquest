use std::{ops::Deref, pin::Pin};

use crate::{
    blocking::handler::BlockingHandler,
    curl_ng::multi::{IsSendWithMultiSet, IsSyncWithMultiSet, MultiEasySet},
    request::{BoxEasyHandle, EasyHandle},
};

type Handle = BoxEasyHandle<BlockingHandler>;

#[derive(Default)]
pub(super) struct SingleMultiSet {
    pub(super) slot: Option<Handle>,
}

unsafe impl IsSendWithMultiSet for SingleMultiSet {}
unsafe impl IsSyncWithMultiSet for SingleMultiSet {}

type Ptr = Box<EasyHandle<BlockingHandler>>;

unsafe impl MultiEasySet for SingleMultiSet {
    type Ptr = Ptr;

    fn add(&mut self, item: Pin<Self::Ptr>) -> usize {
        assert!(self.slot.is_none(), "SingleMultiSet can only hold one item");
        self.slot = Some(item);
        0
    }

    fn remove(&mut self, token: usize) -> Option<Pin<Self::Ptr>> {
        if token != 0 {
            return None;
        }
        self.slot.take()
    }

    unsafe fn lookup(&mut self, token: usize) -> Option<Pin<&mut <Self::Ptr as Deref>::Target>> {
        if token != 0 {
            return None;
        }
        self.slot.as_mut().map(|e| e.as_mut())
    }

    fn is_empty(&self) -> bool {
        self.slot.is_none()
    }

    fn shrink_to_fit(&mut self) {}

    type IterMut<'s> = std::iter::Enumerate<std::option::IterMut<'s, Pin<Ptr>>>;
    fn iter_mut<'s>(&'s mut self) -> Self::IterMut<'s> {
        self.slot.iter_mut().enumerate()
    }
}
