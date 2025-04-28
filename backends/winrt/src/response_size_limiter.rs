use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use nyquest_interface::{Error as NyquestError, Result as NyquestResult};
use windows_core::RuntimeType;
use windows_future::{AsyncOperationProgressHandler, IAsyncOperationWithProgress};

use crate::error::IntoNyquestResult;

pub(crate) struct ResponseSizeLimiterInner {
    max_size: u64,
    size_exceeded: Arc<AtomicBool>,
}

#[must_use = "call assert_size to enforce response size limit"]
pub(crate) struct ResponseSizeLimiter {
    inner: Option<ResponseSizeLimiterInner>,
}

impl ResponseSizeLimiter {
    pub fn hook_progress<TResult: RuntimeType>(
        max_size: Option<u64>,
        task: &IAsyncOperationWithProgress<TResult, u64>,
    ) -> NyquestResult<Self> {
        let Some(max_size) = max_size else {
            return Ok(Self { inner: None });
        };
        let size_exceeded = Arc::new(AtomicBool::new(false));
        let size_exceeded_cloned = size_exceeded.clone();
        task.SetProgress(&AsyncOperationProgressHandler::new(
            move |task, progress| {
                if *progress > max_size {
                    size_exceeded.store(true, std::sync::atomic::Ordering::SeqCst);
                    task.as_ref().map(|t| t.Cancel().ok());
                }
                Ok(())
            },
        ))
        .into_nyquest_result()?;
        let limiter = Self {
            inner: Some(ResponseSizeLimiterInner {
                max_size,
                size_exceeded: size_exceeded_cloned,
            }),
        };
        Ok(limiter)
    }

    pub fn assert_size<T: AsRef<[u8]>>(self, res: NyquestResult<T>) -> NyquestResult<T> {
        let Some(limiter) = self.inner else {
            return res;
        };
        if limiter.size_exceeded.load(Ordering::SeqCst) {
            return Err(NyquestError::ResponseTooLarge);
        }
        let res = res?;
        if res.as_ref().len() > limiter.max_size as usize {
            return Err(NyquestError::ResponseTooLarge);
        }
        Ok(res)
    }
}
