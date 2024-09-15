use nyquest::Result as NyquestResult;

pub(crate) trait IntoNyquestResult<T> {
    fn into_nyquest_result(self) -> NyquestResult<T>;
}

impl<T, E> IntoNyquestResult<T> for Result<T, E>
where
    std::io::Error: From<E>,
{
    fn into_nyquest_result(self) -> NyquestResult<T> {
        Ok(self.map_err(|e| std::io::Error::from(e))?)
    }
}
