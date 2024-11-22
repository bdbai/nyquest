use windows::core as windows_core;
use windows::core::*;
use windows::Foundation::Collections::{IKeyValuePair, IKeyValuePair_Impl};

#[implement(IKeyValuePair<HSTRING, HSTRING>)]
pub(crate) struct StringPair(pub(crate) HSTRING, pub(crate) HSTRING);

#[allow(non_snake_case)]
impl IKeyValuePair_Impl<HSTRING, HSTRING> for StringPair_Impl {
    fn Key(&self) -> windows_core::Result<HSTRING> {
        Ok(self.0.clone())
    }

    fn Value(&self) -> windows_core::Result<HSTRING> {
        Ok(self.1.clone())
    }
}
