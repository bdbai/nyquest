use std::io;

use nyquest_interface::client::ClientOptions;
use windows::core::{h, HSTRING};
use windows::Web::Http::Filters::HttpBaseProtocolFilter;
use windows::Web::Http::HttpClient;

pub(crate) trait WinrtClientExt: Sized {
    fn create(options: ClientOptions) -> io::Result<Self>;
}

impl WinrtClientExt for HttpClient {
    fn create(options: ClientOptions) -> io::Result<Self> {
        let filter = HttpBaseProtocolFilter::new()?;
        filter.SetAutomaticDecompression(true)?;
        let client = HttpClient::Create(&filter)?;
        if let Some(user_agent) = &options.user_agent {
            client
                .DefaultRequestHeaders()?
                .Append(h!("user-agent"), &HSTRING::from(user_agent))?;
        }
        for (name, value) in options.default_headers.iter() {
            client
                .DefaultRequestHeaders()?
                .Append(&HSTRING::from(name), &HSTRING::from(value))?;
        }
        // TODO: options
        Ok(client)
    }
}
