use std::sync::OnceLock;

#[cfg(feature = "async")]
pub(crate) static ASYNC_BACKEND_INSTANCE: OnceLock<Box<dyn super::AnyAsyncBackend>> =
    OnceLock::new();
#[cfg(feature = "async")]
pub fn register_async_backend(backend: impl super::AsyncBackend) {
    ASYNC_BACKEND_INSTANCE
        .set(Box::new(backend))
        .map_err(|_| ())
        .expect("nyquest async backend already registered");
}

#[cfg(feature = "blocking")]
pub fn register_blocking_backend(backend: impl super::BlockingBackend) {
    todo!()
}
