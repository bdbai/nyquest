use std::io;

use nyquest::blocking::backend::{BlockingBackend, BlockingClient, BlockingResponse};
use nyquest::client::{BuildClientResult, ClientOptions};
use nyquest::{Error as NyquestError, Request, Result as NyquestResult};
use windows::core::{Interface, HSTRING};
use windows::Foundation::Uri;
use windows::Web::Http::{HttpClient, HttpMethod, HttpRequestMessage};
use windows::Win32::System::WinRT::IBufferByteAccess;

use crate::error::IntoNyquestResult;
use crate::response::WinrtResponse;

#[derive(Clone)]
pub struct WinrtBlockingBackend;
#[derive(Clone)]
pub struct WinrtBlockingClient {
    base_url: Option<HSTRING>,
    client: HttpClient,
}

impl WinrtBlockingBackend {
    pub fn create_client(&self, options: ClientOptions) -> io::Result<WinrtBlockingClient> {
        let client = HttpClient::new()?;
        let base_url = options.base_url.as_ref().map(|s| HSTRING::from(s));
        Ok(WinrtBlockingClient { base_url, client })
    }
}

impl WinrtBlockingClient {
    fn build_uri(&self, relative: &str) -> io::Result<Uri> {
        let uri = if let Some(base_url) = &self.base_url {
            Uri::CreateWithRelativeUri(base_url, &HSTRING::from(relative))?
        } else {
            Uri::CreateUri(&HSTRING::from(relative))?
        };
        Ok(uri)
    }
    fn send_request(&self, uri: &Uri, req: Request) -> io::Result<WinrtResponse> {
        let req_msg = HttpRequestMessage::new()?;
        // TODO: cache method
        req_msg.SetMethod(&HttpMethod::Create(&HSTRING::from(req.method)).unwrap())?;
        req_msg.SetRequestUri(uri)?;
        let res = self.client.SendRequestAsync(&req_msg)?.get()?;
        Ok(WinrtResponse {
            response: res,
            reader: None,
        })
    }
}

impl BlockingClient for WinrtBlockingClient {
    type Response = WinrtResponse;
    fn request(&self, req: Request) -> NyquestResult<Self::Response> {
        let uri = self
            .build_uri(&req.relative_uri)
            .map_err(|_| NyquestError::InvalidUrl)?;
        self.send_request(&uri, req).into_nyquest_result()
    }
}

impl BlockingBackend for WinrtBlockingBackend {
    type BlockingClient = WinrtBlockingClient;

    fn create_blocking_client(
        &self,
        options: ClientOptions,
    ) -> BuildClientResult<Self::BlockingClient> {
        Ok(self.create_client(options).into_nyquest_result()?)
    }
}

impl BlockingResponse for WinrtResponse {
    fn status(&self) -> u16 {
        self.response
            .StatusCode()
            .expect("failed to get Windows.Web.Http.HttpResponseMessage.StatusCode")
            .0 as _
    }

    fn get_header(&self, header: &str) -> NyquestResult<Vec<String>> {
        let headers = self.response.Headers().into_nyquest_result()?;
        let header_name = HSTRING::from(header);
        let mut headers = headers.Lookup(&header_name).ok();
        if headers.is_none() {
            headers = self
                .content()
                .into_nyquest_result()?
                .Headers()
                .into_nyquest_result()?
                .Lookup(&header_name)
                .ok();
        }
        Ok(headers.into_iter().map(|h| h.to_string_lossy()).collect())
    }

    fn content_length(&self) -> NyquestResult<Option<u64>> {
        let mut len = 0;
        let res = self
            .content()
            .into_nyquest_result()?
            .TryComputeLength(&mut len)
            .into_nyquest_result()?;
        Ok(res.then_some(len))
    }

    fn text(&mut self) -> NyquestResult<String> {
        let content = self
            .content()
            .into_nyquest_result()?
            .ReadAsStringAsync()
            .into_nyquest_result()?
            .get()
            .into_nyquest_result()?;
        Ok(content.to_string_lossy())
    }

    fn bytes(&mut self) -> NyquestResult<Vec<u8>> {
        let content = self
            .content()
            .into_nyquest_result()?
            .ReadAsBufferAsync()
            .into_nyquest_result()?
            .get()
            .into_nyquest_result()?;
        let iba = content.cast::<IBufferByteAccess>().into_nyquest_result()?;
        let arr = unsafe {
            let len = content.Length().into_nyquest_result()? as usize;
            let ptr = iba.Buffer().into_nyquest_result()?;
            let mut arr = Vec::with_capacity(len);
            std::ptr::copy_nonoverlapping(ptr, arr.as_mut_ptr(), len);
            arr.set_len(len);
            arr
        };
        Ok(arr)
    }
}

impl io::Read for WinrtResponse {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let reader = self.reader_mut()?;

        let mut size = reader.UnconsumedBufferLength()?;
        if size == 0 {
            let loaded = reader.LoadAsync(buf.len() as u32)?.get()?;
            if loaded == 0 {
                return Ok(0);
            }
            size = reader.UnconsumedBufferLength()?;
        }
        let size = buf.len().min(size as usize);
        let buf = &mut buf[..size];
        reader.ReadBytes(buf)?;
        Ok(size)
    }
}
