//! Backend registration functionality for nyquest.
//!
//! This module provides the mechanism for registering a backend implementation
//! to be used by the nyquest facade.
//!
//! Backend implementations should create a single type that satisfies the appropriate
//! traits based on the enabled features (`AsyncBackend` when the `async` feature is enabled
//! and/or `BlockingBackend` when the `blocking` feature is enabled).

mod __priv {
    use cfg_if::cfg_if;

    cfg_if! {
        if #[cfg(feature = "async")] {
            /// Trait for backends that may implement async functionality.
            ///
            /// Automatically implemented for any type that implements `AnyAsyncBackend`.
            pub trait MaybeAsync: crate::r#async::AnyAsyncBackend {}
            impl<B: crate::r#async::AnyAsyncBackend> MaybeAsync for B {}
        } else {
            /// Trait for backends when async functionality is not required.
            pub trait MaybeAsync {}
            impl<B> MaybeAsync for B {}
        }
    }

    cfg_if! {
        if #[cfg(feature = "blocking")] {
            /// Trait for backends that may implement blocking functionality.
            ///
            /// Automatically implemented for any type that implements `AnyBlockingBackend`.
            pub trait MaybeBlocking: crate::blocking::AnyBlockingBackend {}
            impl<B: crate::blocking::AnyBlockingBackend> MaybeBlocking for B {}
        } else {
            /// Trait for backends when blocking functionality is not required.
            pub trait MaybeBlocking {}
            impl<B> MaybeBlocking for B {}
        }
    }

    /// Trait for backends that can be registered with nyquest.
    ///
    /// Automatically implemented for types that satisfy the `MaybeAsync`
    /// and `MaybeBlocking` constraints.
    pub trait RegisterBackend: MaybeAsync + MaybeBlocking {}
    impl<B: MaybeAsync + MaybeBlocking> RegisterBackend for B {}
}

use std::sync::OnceLock;

use __priv::RegisterBackend;

/// Global storage for the registered backend.
///
/// This is used internally by nyquest to access the registered backend implementation.
pub static BACKEND: OnceLock<Box<dyn RegisterBackend + Send + Sync>> = OnceLock::new();

/// Registers a backend implementation for use with nyquest.
///
/// This function should be called once at the beginning of a program to set up
/// the backend that will be used by all nyquest client operations.
///
/// Backend developers should create a type that implements:
/// - [`AsyncBackend`] trait if the `async` feature is enabled
/// - [`BlockingBackend`] trait if the `blocking` feature is enabled
///
/// # Panics
///
/// Panics if a backend has already been registered.
///
/// [`AsyncBackend`]: crate::async::AsyncBackend
/// [`BlockingBackend`]: crate::blocking::BlockingBackend
pub fn register_backend(backend: impl RegisterBackend + Send + Sync + 'static) {
    if BACKEND.set(Box::new(backend)).is_err() {
        panic!("Backend already registered");
    }
}
