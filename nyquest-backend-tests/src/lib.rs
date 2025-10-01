#![cfg(test)]

use std::{
    collections::BTreeMap,
    convert::Infallible,
    future::Future,
    io,
    net::SocketAddr,
    pin::Pin,
    sync::{LazyLock, Mutex, Once},
};

use http_body_util::{BodyExt, Full};
use hyper::{
    body::{self, Bytes},
    server::conn::http1,
    service::service_fn,
    Request, Response,
};
use hyper_util::rt::TokioIo;
use nyquest::ClientBuilder;
use tokio::net::TcpListener;

mod fixtures;
mod request_ext;

pub use request_ext::RequestExt;

#[must_use]
struct HyperFixtureHandle(String);

impl Drop for HyperFixtureHandle {
    fn drop(&mut self) {
        let failed_request = {
            let mut services = HYPER_SERVICE_FIXTURES.lock().unwrap();
            services
                .get_mut(&*self.0)
                .expect("fixture not found")
                .assertion_failed_request
                .take()
        };
        if let Some(req) = failed_request {
            panic!("assertion failed for request {}: {:?}", self.0, req);
        }
    }
}

// BoxedBody for supporting streaming/chunked responses
type BoxedBody = http_body_util::combinators::BoxBody<Bytes, hyper::Error>;

// New type allowing both Full<Bytes> and BoxedBody response types
// This makes existing tests compatible with the new changes
type FixtureAssertionResult = (ResponseWrapper, Result<(), Request<body::Incoming>>);

struct ResponseWrapper(Response<BoxedBody>);

impl From<Response<Full<Bytes>>> for ResponseWrapper {
    fn from(resp: Response<Full<Bytes>>) -> Self {
        let resp = resp.map(|body| body.map_err(|_| unreachable!()).boxed());
        ResponseWrapper(resp)
    }
}

impl From<Response<BoxedBody>> for ResponseWrapper {
    fn from(resp: Response<BoxedBody>) -> Self {
        ResponseWrapper(resp)
    }
}

type HyperServiceFixtureCallback = Box<
    dyn Fn(Request<body::Incoming>) -> Pin<Box<dyn Future<Output = FixtureAssertionResult> + Send>>
        + Send
        + Sync,
>;
struct HyperServiceFixture {
    svc: HyperServiceFixtureCallback,
    assertion_failed_request: Option<Request<body::Incoming>>,
}

static HYPER_SERVICE_FIXTURES: Mutex<BTreeMap<String, HyperServiceFixture>> =
    Mutex::new(BTreeMap::new());

fn add_hyper_fixture<Fut, Resp>(
    url: impl Into<String>,
    svc_fn: impl Fn(Request<body::Incoming>) -> Fut + Send + Sync + 'static,
) -> HyperFixtureHandle
where
    Fut: Future<Output = (Resp, Result<(), Request<body::Incoming>>)> + Send + 'static,
    Resp: Into<ResponseWrapper>,
{
    let mut url: String = url.into();
    if !url.starts_with('/') {
        url.insert(0, '/');
    }
    let svc = Box::new(move |req| {
        let fut = svc_fn(req);
        Box::pin(async move {
            let (resp, result) = fut.await;
            (resp.into(), result)
        }) as _
    });
    let fixture = HyperServiceFixture {
        svc,
        assertion_failed_request: None,
    };
    {
        let url = url.clone();
        let mut services = HYPER_SERVICE_FIXTURES.lock().unwrap();
        services.insert(url, fixture);
    }
    HyperFixtureHandle(url)
}

async fn handle_service(req: Request<body::Incoming>) -> Result<Response<BoxedBody>, Infallible> {
    let path = req.uri().path().to_owned();
    let fut = {
        let services = HYPER_SERVICE_FIXTURES.lock().unwrap();
        let fixture = services.get(&*path).unwrap();
        (fixture.svc)(req)
    };
    let (response, result) = fut.await;

    if let Err(req) = result {
        let mut services = HYPER_SERVICE_FIXTURES.lock().unwrap();
        let fixture = services.get_mut(&*path).unwrap();
        fixture.assertion_failed_request = Some(req);
    }

    Ok(response.0)
}

async fn setup_hyper_impl() -> Result<String, io::Error> {
    // TODO: handle panic
    let addr = SocketAddr::from(([127, 0, 0, 1], 0));

    let listener = TcpListener::bind(addr).await?;
    let port = listener.local_addr()?.port();

    tokio::spawn(async move {
        loop {
            let (stream, _) = listener.accept().await.expect("accept failed");
            let io = TokioIo::new(stream);

            tokio::task::spawn(async move {
                if let Err(err) = http1::Builder::new()
                    // `service_fn` converts our function in a `Service`
                    .serve_connection(io, service_fn(handle_service))
                    .await
                {
                    eprintln!("Error serving connection: {err:?}");
                }
            });
        }
    });

    Ok(format!("http://127.0.0.1:{port}"))
}

static TOKIO_RT: LazyLock<tokio::runtime::Runtime> = LazyLock::new(|| {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap()
});

async fn init_builder() -> io::Result<ClientBuilder> {
    use tokio::sync::OnceCell;

    static BACKEND_INIT: Once = Once::new();
    BACKEND_INIT.call_once(init_backend);

    static HYPER_SERVICE_INIT: OnceCell<io::Result<String>> = OnceCell::const_new();
    match HYPER_SERVICE_INIT.get_or_init(setup_hyper_impl).await {
        Ok(url) => Ok(ClientBuilder::default().base_url(url.clone())),
        Err(err) => Err(io::Error::new(err.kind(), err.to_string())),
    }
}

fn init_builder_blocking() -> io::Result<ClientBuilder> {
    TOKIO_RT.block_on(async {
        init_builder()
            .await
            .map(|cb| cb.with_header("blocking", "1"))
    })
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
        ("reqwest", nyquest_backend_reqwest),
    );
}
