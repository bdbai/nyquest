use std::future::{Future, IntoFuture};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use nyquest_interface::{Error as NyquestError, Result as NyquestResult};
use windows::System::Threading::{ThreadPoolTimer, TimerElapsedHandler};

use crate::error::IntoNyquestResult;
use crate::timer::{Cancel, Timer};

pub(crate) trait AsyncTimeoutExt {
    type Output;
    fn timeout_by(self, timer: &mut Timer) -> impl Future<Output = NyquestResult<Self::Output>>;
}

impl<T, F> AsyncTimeoutExt for F
where
    F: IntoFuture<Output = windows_core::Result<T>> + Clone + Cancel + Send + 'static,
{
    type Output = T;
    async fn timeout_by(self, timer: &mut Timer) -> NyquestResult<Self::Output> {
        let Some(remaining) = &mut timer.remaining else {
            // TODO: cancel on drop
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
