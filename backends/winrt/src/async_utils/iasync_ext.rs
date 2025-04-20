use std::io;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::{future::Future, pin::Pin};

use futures_util::task::AtomicWaker;
use windows::core::{Interface, RuntimeType};
use windows::Foundation::{
    AsyncStatus, IAsyncAction, IAsyncActionWithProgress, IAsyncInfo, IAsyncOperation,
    IAsyncOperationWithProgress,
};

use super::iasync_like::IAsyncLike;

pub(crate) struct IAsyncFut<A> {
    inner: A,
    status: IAsyncInfo,
    waker: Arc<AtomicWaker>,
}

pub(crate) trait IAsyncExt: Sized {
    fn wait(self) -> io::Result<IAsyncFut<Self>>;
}

impl<A: Interface + IAsyncLike> IAsyncExt for A {
    fn wait(self) -> io::Result<IAsyncFut<Self>> {
        let status = self.cast()?;
        let waker = Arc::new(AtomicWaker::new());
        self.set_callback({
            let waker = waker.clone();
            move || waker.wake()
        })?;
        Ok(IAsyncFut {
            inner: self,
            status,
            waker,
        })
    }
}

impl<A> IAsyncFut<A> {
    fn is_running(&self) -> io::Result<bool> {
        Ok(self.status.Status()? == AsyncStatus::Started)
    }
}

impl<A: IAsyncLike> Future for IAsyncFut<A> {
    type Output = io::Result<A::Output>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if !self.is_running()? {
            return Poll::Ready(self.inner.get_output());
        }
        self.waker.register(cx.waker());
        Poll::Pending
    }
}

impl<A> Drop for IAsyncFut<A> {
    fn drop(&mut self) {
        if self.is_running().unwrap_or_default() {
            self.status.Cancel().ok();
        }
    }
}

unsafe impl Send for IAsyncFut<IAsyncAction> {}
unsafe impl<P: RuntimeType> Send for IAsyncFut<IAsyncActionWithProgress<P>> {}
unsafe impl<R: RuntimeType> Send for IAsyncFut<IAsyncOperation<R>> {}
unsafe impl<R: RuntimeType, P: RuntimeType> Send for IAsyncFut<IAsyncOperationWithProgress<R, P>> {}
