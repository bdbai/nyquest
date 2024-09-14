use cfg_if::cfg_if;

mod error;
mod options;
pub(crate) mod register;

cfg_if! {
    if #[cfg(feature = "async")] {
        mod r#async;
        pub(crate) use r#async::{AnyAsyncBackend, AnyAsyncClient};
        pub use r#async::{AsyncBackend, AsyncClient};
        pub use register::register_async_backend;
    }
}

cfg_if! {
    if #[cfg(feature = "blocking")] {
        mod blocking;
        pub(crate) use blocking::AnyBlockingBackend;
        pub use blocking::{BlockingBackend, BlockingClient};
        pub use register::register_blocking_backend;
    }
}

pub use error::{BackendError, BackendResult};
pub use options::ClientOptions;
