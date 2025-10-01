use std::{
    future::Future,
    ops::{Deref, DerefMut},
};

pub struct SendWrapper<T> {
    inner: T,
}

impl<T> SendWrapper<T> {
    pub fn new(inner: T) -> Self {
        Self { inner }
    }

    pub fn into_inner(self) -> T {
        self.inner
    }
}

// FIXME: wasm with atomic support
#[cfg(target_arch = "wasm32")]
unsafe impl<T> Send for SendWrapper<T> {}
#[cfg(target_arch = "wasm32")]
unsafe impl<T> Sync for SendWrapper<T> {}

impl<T> Deref for SendWrapper<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<T> DerefMut for SendWrapper<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

impl<T: Future> Future for SendWrapper<T> {
    type Output = T::Output;

    fn poll(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        // Safety: We never move the inner future.
        unsafe { std::pin::Pin::new_unchecked(&mut self.get_unchecked_mut().inner) }.poll(cx)
    }
}
