use std::future::Future;
use std::ops::Deref;
use std::pin::Pin;
use std::task::{Context, Poll};

use crate::error::WinHttpError;
use crate::r#async::context::{RequestContext, RequestState};

pub struct StateFuture<C> {
    context: C,
    expected_states: RequestState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateChangedResult {
    pub state: RequestState,
    pub bytes_transferred: usize,
}

impl<C: Deref<Target = RequestContext>> Future for StateFuture<C> {
    type Output = Result<StateChangedResult, WinHttpError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        poll_for_state(&self.context, self.expected_states, cx)
    }
}

fn poll_for_state(
    context: &RequestContext,
    expected_states: RequestState,
    cx: &mut Context<'_>,
) -> Poll<Result<StateChangedResult, WinHttpError>> {
    if let Some(error) = context.take_error() {
        return Poll::Ready(Err(error));
    }
    let mut inner = context.inner.lock().unwrap();
    let state = inner.state;
    if state.intersects(expected_states) {
        Poll::Ready(Ok(StateChangedResult {
            state,
            bytes_transferred: inner.buffer_range.end,
        }))
    } else {
        inner.waker.clone_from(cx.waker());
        Poll::Pending
    }
}

pub(super) fn wait_for_state<C>(context: C, expected_states: RequestState) -> StateFuture<C> {
    StateFuture {
        context,
        expected_states,
    }
}
