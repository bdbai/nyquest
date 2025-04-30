use std::future::{Future, IntoFuture};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use nyquest_interface::{Error as NyquestError, Result as NyquestResult};
use windows::System::Threading::{ThreadPoolTimer, TimerElapsedHandler};
use windows_core::RuntimeType;
use windows_future::IAsyncOperationWithProgress;

use crate::error::IntoNyquestResult;

pub(crate) struct Timer {
    remaining: Option<Duration>,
}

impl Timer {
    pub(crate) fn new(timeout: Option<Duration>) -> Self {
        Self { remaining: timeout }
    }
}

trait Cancel {
    fn cancel(&self) -> windows_core::Result<()>;
}
trait BlockingGet {
    type Output;
    fn get(self) -> Self::Output;
}
pub(crate) trait AsyncTimeoutExt {
    type Output;
    fn timeout_by(self, timer: &mut Timer) -> impl Future<Output = NyquestResult<Self::Output>>;
}
pub(crate) trait BlockingTimeoutExt {
    type Output;
    fn timeout_by(self, timer: &mut Timer) -> NyquestResult<Self::Output>;
}

impl<T: RuntimeType, P: RuntimeType> Cancel for IAsyncOperationWithProgress<T, P> {
    fn cancel(&self) -> windows_core::Result<()> {
        self.Cancel()
    }
}
impl<T: RuntimeType, P: RuntimeType> BlockingGet for IAsyncOperationWithProgress<T, P> {
    type Output = windows_core::Result<T>;
    fn get(self) -> Self::Output {
        IAsyncOperationWithProgress::get(&self)
    }
}

impl<T, F> AsyncTimeoutExt for F
where
    F: IntoFuture<Output = windows_core::Result<T>> + Clone + Cancel + Send + 'static,
{
    type Output = T;
    async fn timeout_by(self, timer: &mut Timer) -> NyquestResult<Self::Output> {
        let Some(remaining) = &mut timer.remaining else {
            return self.await.into_nyquest_result();
        };
        if remaining.is_zero() {
            return Err(NyquestError::RequestTimeout);
        }
        let instant_before = Instant::now();
        let cancelled = Arc::new(AtomicBool::new(false));
        let timer = ThreadPoolTimer::CreateTimer(
            &TimerElapsedHandler::new({
                let task = self.clone();
                let cancelled = cancelled.clone();
                move |_| {
                    cancelled.store(true, Ordering::SeqCst);
                    task.cancel().ok();
                    Ok(())
                }
            }),
            (*remaining).into(),
        )
        .into_nyquest_result()?;
        let res = self.await; // TODO: select on the timer
        timer.Cancel().ok();
        if cancelled.load(Ordering::SeqCst) {
            return Err(NyquestError::RequestTimeout);
        }
        *remaining = remaining.saturating_sub(instant_before.elapsed());
        res.into_nyquest_result()
    }
}

impl<T, F> BlockingTimeoutExt for F
where
    F: BlockingGet<Output = windows_core::Result<T>> + Clone + Cancel + Send + 'static,
{
    type Output = T;
    fn timeout_by(self, timer: &mut Timer) -> NyquestResult<Self::Output> {
        let Some(remaining) = &mut timer.remaining else {
            return self.get().into_nyquest_result();
        };
        if remaining.is_zero() {
            return Err(NyquestError::RequestTimeout);
        }
        let instant_before = Instant::now();
        let cancelled = Arc::new(AtomicBool::new(false));
        let timer = ThreadPoolTimer::CreateTimer(
            &TimerElapsedHandler::new({
                let task = self.clone();
                let cancelled = cancelled.clone();
                move |_| {
                    cancelled.store(true, Ordering::SeqCst);
                    task.cancel().ok();
                    Ok(())
                }
            }),
            (*remaining).into(),
        )
        .into_nyquest_result()?;
        let res = self.get();
        timer.Cancel().ok();
        if cancelled.load(Ordering::SeqCst) {
            return Err(NyquestError::RequestTimeout);
        }
        *remaining = remaining.saturating_sub(instant_before.elapsed());
        res.into_nyquest_result()
    }
}
