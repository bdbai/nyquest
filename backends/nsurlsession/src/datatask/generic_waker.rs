use std::sync::Arc;

#[cfg(feature = "async")]
type AsyncWaker = crate::r#async::waker::AsyncWaker;
#[cfg(feature = "blocking")]
type BlockingWaker = crate::blocking::waker::BlockingWaker;

#[derive(Clone)]
pub(crate) enum GenericWaker {
    #[cfg(feature = "async")]
    Async(Arc<AsyncWaker>),
    #[cfg(feature = "blocking")]
    Blocking(Arc<BlockingWaker>),
}

impl GenericWaker {
    pub(crate) fn wake(&self) {
        match self {
            #[cfg(feature = "async")]
            GenericWaker::Async(waker) => waker.wake(),
            #[cfg(feature = "blocking")]
            GenericWaker::Blocking(waker) => waker.wake(),
        }
    }
}
