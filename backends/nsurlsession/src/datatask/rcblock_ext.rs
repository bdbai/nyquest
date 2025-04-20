use std::ops::Deref;

use arc_swap::RefCnt;
use block2::{Block, RcBlock};

pub(super) struct SwappableRcBlock<T: ?Sized>(RcBlock<T>);

impl<T: ?Sized> Clone for SwappableRcBlock<T> {
    fn clone(&self) -> Self {
        SwappableRcBlock(self.0.clone())
    }
}

impl<T: ?Sized> From<RcBlock<T>> for SwappableRcBlock<T> {
    fn from(block: RcBlock<T>) -> Self {
        SwappableRcBlock(block)
    }
}

impl<T: ?Sized> Deref for SwappableRcBlock<T> {
    type Target = RcBlock<T>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

unsafe impl<T: ?Sized> RefCnt for SwappableRcBlock<T> {
    type Base = Block<T>;

    fn into_ptr(me: Self) -> *mut Self::Base {
        RcBlock::into_raw(me.0)
    }

    fn as_ptr(me: &Self) -> *mut Self::Base {
        RcBlock::as_ptr(&me.0)
    }

    unsafe fn from_ptr(ptr: *const Self::Base) -> Self {
        let block = RcBlock::from_raw(ptr as *mut _);
        // Safety: `ptr` cannot be null, as the `RcBlock` that has been converted to the
        // raw pointer is guaranteed to be non-null.
        SwappableRcBlock(block.unwrap_unchecked())
    }
}
