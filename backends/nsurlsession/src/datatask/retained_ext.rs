use std::ops::Deref;

use arc_swap::RefCnt;
use objc2::{rc::Retained, Message};

pub(super) struct SwappableRetained<T: ?Sized>(pub(super) Retained<T>);

impl<T: Message> Clone for SwappableRetained<T> {
    fn clone(&self) -> Self {
        SwappableRetained(self.0.clone())
    }
}

impl<T: ?Sized> From<Retained<T>> for SwappableRetained<T> {
    fn from(block: Retained<T>) -> Self {
        SwappableRetained(block)
    }
}

impl<T: ?Sized> From<SwappableRetained<T>> for Retained<T> {
    fn from(block: SwappableRetained<T>) -> Self {
        block.0
    }
}

impl<T: ?Sized> Deref for SwappableRetained<T> {
    type Target = Retained<T>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

unsafe impl<T: Message> RefCnt for SwappableRetained<T> {
    type Base = T;

    fn into_ptr(me: Self) -> *mut Self::Base {
        Retained::into_raw(me.0)
    }

    fn as_ptr(me: &Self) -> *mut Self::Base {
        Retained::as_ptr(&me.0) as *mut _
    }

    unsafe fn from_ptr(ptr: *const Self::Base) -> Self {
        let block = Retained::from_raw(ptr as *mut T);
        // Safety: `ptr` cannot be null, as the `Retained` that has been converted to the
        // raw pointer is guaranteed to be non-null.
        SwappableRetained(block.unwrap_unchecked())
    }
}
