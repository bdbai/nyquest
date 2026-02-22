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

impl<C: Deref<Target = RequestContext>> Future for StateFuture<C> {
    type Output = Result<(), WinHttpError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        poll_for_state(&self.context, self.expected_states, cx)
    }
}

fn poll_for_state(
    context: &RequestContext,
    expected_states: RequestState,
    cx: &mut Context<'_>,
) -> Poll<Result<(), WinHttpError>> {
    if let Some(error) = context.take_error() {
        return Poll::Ready(Err(error));
    }
    let state = context.state();
    if state.intersects(expected_states) {
        Poll::Ready(Ok(()))
    } else {
        context.set_waker(cx.waker());
        Poll::Pending
    }
}

pub(super) fn wait_for_state<C>(context: C, expected_states: RequestState) -> StateFuture<C> {
    StateFuture {
        context,
        expected_states,
    }
}
