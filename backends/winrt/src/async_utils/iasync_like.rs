use std::io;

use windows::core::RuntimeType;
use windows::Foundation::{
    AsyncActionCompletedHandler, AsyncActionWithProgressCompletedHandler,
    AsyncOperationCompletedHandler, AsyncOperationWithProgressCompletedHandler, IAsyncAction,
    IAsyncActionWithProgress, IAsyncOperation, IAsyncOperationWithProgress,
};

pub(crate) trait IAsyncLike {
    type Output;
    fn get_output(&self) -> io::Result<Self::Output>;
    fn set_callback<F>(&self, callback: F) -> io::Result<()>
    where
        F: FnMut() + Send + 'static;
}

impl IAsyncLike for IAsyncAction {
    type Output = ();

    fn get_output(&self) -> io::Result<Self::Output> {
        Ok(())
    }
    fn set_callback<F>(&self, mut callback: F) -> io::Result<()>
    where
        F: FnMut() + Send + 'static,
    {
        Ok(
            self.SetCompleted(&AsyncActionCompletedHandler::new(move |_, _| {
                callback();
                Ok(())
            }))?,
        )
    }
}

impl<P: RuntimeType> IAsyncLike for IAsyncActionWithProgress<P> {
    type Output = ();

    fn get_output(&self) -> io::Result<Self::Output> {
        Ok(())
    }
    fn set_callback<F>(&self, mut callback: F) -> io::Result<()>
    where
        F: FnMut() + Send + 'static,
    {
        Ok(
            self.SetCompleted(&AsyncActionWithProgressCompletedHandler::new(
                move |_, _| {
                    callback();
                    Ok(())
                },
            ))?,
        )
    }
}

impl<R: RuntimeType> IAsyncLike for IAsyncOperation<R> {
    type Output = R;

    fn get_output(&self) -> io::Result<Self::Output> {
        Ok(self.GetResults()?)
    }
    fn set_callback<F>(&self, mut callback: F) -> io::Result<()>
    where
        F: FnMut() + Send + 'static,
    {
        Ok(
            self.SetCompleted(&AsyncOperationCompletedHandler::new(move |_, _| {
                callback();
                Ok(())
            }))?,
        )
    }
}

impl<R: RuntimeType, P: RuntimeType> IAsyncLike for IAsyncOperationWithProgress<R, P> {
    type Output = R;

    fn get_output(&self) -> io::Result<Self::Output> {
        Ok(self.GetResults()?)
    }
    fn set_callback<F>(&self, mut callback: F) -> io::Result<()>
    where
        F: FnMut() + Send + 'static,
    {
        Ok(
            self.SetCompleted(&AsyncOperationWithProgressCompletedHandler::new(
                move |_, _| {
                    callback();
                    Ok(())
                },
            ))?,
        )
    }
}
