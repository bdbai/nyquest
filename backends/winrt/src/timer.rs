use std::time::Duration;

use windows_core::RuntimeType;
use windows_future::IAsyncOperationWithProgress;

pub(crate) struct Timer {
    pub(crate) remaining: Option<Duration>,
}

impl Timer {
    pub(crate) fn new(timeout: Option<Duration>) -> Self {
        Self { remaining: timeout }
    }
}

pub(crate) trait Cancel {
    fn cancel(&self) -> windows_core::Result<()>;
}

impl<T: RuntimeType, P: RuntimeType> Cancel for IAsyncOperationWithProgress<T, P> {
    fn cancel(&self) -> windows_core::Result<()> {
        self.Cancel()
    }
}
