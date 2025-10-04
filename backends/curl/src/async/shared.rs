use std::sync::Mutex;

use futures_util::task::AtomicWaker;
use nyquest_interface::Result as NyquestResult;

use crate::{r#async::read_task::SharedStreamState, state::RequestState};

#[derive(Default)]
pub(super) struct SharedRequestStatesInner {
    pub(super) state: RequestState,
    pub(super) req_streams: Vec<SharedStreamState>,
    pub(super) result: RequestResult,
    pub(super) response: Option<super::CurlAsyncResponse>,
}

#[derive(Default)]
pub(super) struct SharedRequestStates {
    pub(super) waker: AtomicWaker,
    pub(super) state: Mutex<SharedRequestStatesInner>,
}

#[derive(Default)]
pub(super) enum RequestResult {
    #[default]
    Init,
    InProgress {
        id: usize,
    },
    EasyLost(super::Easy),
    Done {
        res: NyquestResult<()>,
        id: usize,
    },
}
