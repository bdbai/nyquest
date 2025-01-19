mod __priv {
    use cfg_if::cfg_if;

    cfg_if! {
        if #[cfg(feature = "async")] {
            pub trait MaybeAsync: crate::r#async::any::AnyAsyncBackend {}
            impl<B: crate::r#async::any::AnyAsyncBackend> MaybeAsync for B {}
        } else {
            pub trait MaybeAsync {}
            impl<B> MaybeAsync for B {}
        }
    }

    cfg_if! {
        if #[cfg(feature = "blocking")] {
            pub trait MaybeBlocking: crate::blocking::any::AnyBlockingBackend {}
            impl<B: crate::blocking::any::AnyBlockingBackend> MaybeBlocking for B {}
        } else {
            pub trait MaybeBlocking {}
            impl<B> MaybeBlocking for B {}
        }
    }

    pub trait RegisterBackend: MaybeAsync + MaybeBlocking {}
    impl<B: MaybeAsync + MaybeBlocking> RegisterBackend for B {}
}

use std::sync::OnceLock;

use __priv::RegisterBackend;

pub(crate) static BACKEND: OnceLock<Box<dyn RegisterBackend + Send + Sync>> = OnceLock::new();

pub fn register_backend(backend: impl RegisterBackend + Send + Sync) {
    if BACKEND.set(Box::new(backend)).is_err() {
        panic!("Backend already registered");
    }
}
