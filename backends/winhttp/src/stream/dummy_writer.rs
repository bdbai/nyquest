//! Dummy stream writer for when stream features are disabled.

use std::convert::Infallible;
use std::marker::PhantomData;

pub enum StreamWriter<S> {
    _Infallible(Infallible, PhantomData<S>),
}

impl<S> StreamWriter<S> {}
