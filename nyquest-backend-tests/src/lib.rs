#![cfg(test)]

use std::{
    future::Future,
    io,
    sync::{LazyLock, Mutex, Once},
};

use http_body_util::{BodyExt, Full};
use hyper::{
    body::{self, Bytes},
    Request, Response,
};
use nyquest::ClientBuilder;
use tokio::sync::OnceCell;

mod fixtures;
mod hyper_fixture_collection;
mod request_ext;

use hyper_fixture_collection::FixtureAssertionResult;
pub use request_ext::RequestExt;

use crate::hyper_fixture_collection::HyperFixtureCollection;

static TOKIO_RT: LazyLock<tokio::runtime::Runtime> = LazyLock::new(|| {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap()
});

static MAIN_HYPER_FIXTURE_COLLECTION: HyperFixtureCollection = HyperFixtureCollection::new();

async fn init_main_service_port() -> io::Result<u16> {
    static HYPER_SERVICE_INIT: OnceCell<io::Result<u16>> = OnceCell::const_new();
    match HYPER_SERVICE_INIT
        .get_or_init(|| async {
            let port =
                hyper_fixture_collection::spawn_service(&MAIN_HYPER_FIXTURE_COLLECTION).await?;
            Ok(port)
        })
        .await
    {
        Ok(url) => Ok(*url),
        Err(err) => Err(io::Error::new(err.kind(), err.to_string())),
    }
}

async fn init_builder() -> io::Result<ClientBuilder> {
    static BACKEND_INIT: Once = Once::new();
    BACKEND_INIT.call_once(init_backend);

    let port = init_main_service_port().await?;
    let base_url = format!("http://localhost.:{port}"); // Use localhost. to avoid potential proxy bypass due to "localhost" being a special domain in some environments
    Ok(ClientBuilder::default().base_url(base_url))
}

fn init_builder_blocking() -> io::Result<ClientBuilder> {
    TOKIO_RT.block_on(async {
        init_builder()
            .await
            .map(|cb| cb.with_header("blocking", "1"))
    })
}

fn add_hyper_fixture<Fut, Resp>(
    path: impl Into<String>,
    svc_fn: impl Fn(Request<body::Incoming>) -> Fut + Send + Sync + 'static,
) -> hyper_fixture_collection::HyperFixtureHandle<&'static HyperFixtureCollection>
where
    Fut: Future<Output = (Resp, Result<(), Request<body::Incoming>>)> + Send + 'static,
    Resp: Into<hyper_fixture_collection::ResponseWrapper>,
{
    hyper_fixture_collection::add_hyper_fixture(&MAIN_HYPER_FIXTURE_COLLECTION, path, svc_fn)
}

macro_rules! declare_backends {
    ($(($feature:expr, $pkg:ident)),* $(,)*) => {
        cfg_if::cfg_if! {
            if #[cfg(any())] {
            } $(
                else if #[cfg(feature = $feature)] {
                    use $pkg as backend;
                }
            )* else {
                pub mod backend {
                    pub fn register() { }
                }
            }
        }

        #[allow(non_upper_case_globals)]
        let backend_feature_count = 0 $(+ cfg!(feature = $feature) as u32)*;
        match backend_feature_count {
            0 => panic!("No backend feature enabled."),
            1 => backend::register(),
            _ => panic!("Multiple backend features enabled."),
        }
    };
}

fn init_backend() {
    declare_backends!(
        ("curl", nyquest_backend_curl),
        ("nsurlsession", nyquest_backend_nsurlsession),
        ("winrt", nyquest_backend_winrt),
        ("winhttp", nyquest_backend_winhttp),
        ("reqwest", nyquest_backend_reqwest),
    );
}
