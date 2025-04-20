use std::sync::Mutex;
use std::thread::{Thread, ThreadId};

pub(crate) struct BlockingWaker {
    initial_thread_id: ThreadId,
    thread: Mutex<Thread>,
}

impl BlockingWaker {
    pub(crate) fn new_from_current_thread() -> Self {
        let initial_thread_id = std::thread::current().id();
        let thread = std::thread::current();
        BlockingWaker {
            initial_thread_id,
            thread: Mutex::new(thread),
        }
    }

    pub(crate) fn wake(&self) {
        self.thread.lock().unwrap().unpark();
    }

    pub(super) fn register_current_thread(&self) {
        let current_thread = std::thread::current();
        if current_thread.id() != self.initial_thread_id {
            *self.thread.lock().unwrap() = current_thread;
        }
    }
}
