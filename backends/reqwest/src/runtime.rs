use std::{mem::ManuallyDrop, sync::OnceLock};

use tokio::runtime::{Handle as TokioHandle, Runtime as TokioRuntime, RuntimeFlavor};

#[derive(Debug)]
enum ManagedRuntimeKind {
    Managed(ManuallyDrop<TokioRuntime>),
    Ambient(TokioHandle),
}

#[derive(Debug, Default)]
pub(crate) struct ManagedRuntime {
    inner: OnceLock<ManagedRuntimeKind>,
}

impl ManagedRuntimeKind {
    fn handle(&self) -> &TokioHandle {
        match self {
            ManagedRuntimeKind::Managed(runtime) => runtime.handle(),
            ManagedRuntimeKind::Ambient(handle) => handle,
        }
    }
}

impl ManagedRuntime {
    fn get_runtime(&self, new_thread_name: &str) -> &ManagedRuntimeKind {
        self.inner.get_or_init(|| {
            if let Some(handle) = get_current_multithread_runtime_handle() {
                // If we are already in a tokio runtime, we can use its handle directly.
                // The handle will be reused for all operations initiated from this ManagedRuntime instance.
                //
                // If the current runtime is dropped during future execution, we have no choice but to let it fail.
                // There could be IO operations tied to that specific runtime.
                return ManagedRuntimeKind::Ambient(handle);
            }

            let runtime = tokio::runtime::Builder::new_multi_thread()
                .thread_name(new_thread_name)
                .worker_threads(1)
                .enable_all()
                .build()
                .expect("Failed to create managed tokio runtime");
            ManagedRuntimeKind::Managed(ManuallyDrop::new(runtime))
        })
    }

    #[cfg(feature = "blocking")]
    pub(crate) fn get_handle_for_blocking(&self) -> TokioHandle {
        self.get_runtime("nyquest-reqwest-blocking")
            .handle()
            .clone()
    }

    #[cfg(feature = "async")]
    pub(crate) fn get_handle_for_async(&self) -> TokioHandle {
        self.get_runtime("nyquest-reqwest-async").handle().clone()
    }
}

impl Drop for ManagedRuntimeKind {
    fn drop(&mut self) {
        match self {
            ManagedRuntimeKind::Managed(runtime) => {
                // If we are in a tokio runtime, we cannot block on the shutdown of the managed runtime.
                // Instead, we spawn a background task to shut it down asynchronously.
                let runtime = unsafe { ManuallyDrop::take(runtime) };
                if TokioHandle::try_current().is_ok() {
                    std::thread::spawn(move || {
                        drop(runtime);
                    });
                } else {
                    drop(runtime);
                }
            }
            ManagedRuntimeKind::Ambient(_) => {}
        }
    }
}

fn get_current_multithread_runtime_handle() -> Option<TokioHandle> {
    let handle = TokioHandle::try_current().ok()?;
    if handle.runtime_flavor() == RuntimeFlavor::MultiThread {
        return Some(handle);
    }
    None
}
