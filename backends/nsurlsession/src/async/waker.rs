use std::task::Context;

use futures_util::task::AtomicWaker;

pub(crate) struct AsyncWaker {
    waker: AtomicWaker,
}

impl AsyncWaker {
    pub(crate) fn new() -> Self {
        AsyncWaker {
            waker: AtomicWaker::new(),
        }
    }

    pub(super) fn register(&self, cx: &Context<'_>) {
        self.waker.register(cx.waker());
    }

    pub(crate) fn wake(&self) {
        self.waker.wake();
    }
}
