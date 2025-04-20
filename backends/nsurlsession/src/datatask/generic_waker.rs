#[cfg(feature = "async")]
type AsyncWaker = crate::r#async::waker::AsyncWaker;
#[cfg(feature = "blocking")]
type BlockingWaker = crate::blocking::waker::BlockingWaker;

pub(crate) enum GenericWaker {
    #[cfg(feature = "async")]
    Async(AsyncWaker),
    #[cfg(feature = "blocking")]
    Blocking(BlockingWaker),
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
