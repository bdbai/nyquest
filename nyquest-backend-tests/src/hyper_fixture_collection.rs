use std::{
    collections::BTreeMap, convert::Infallible, future::Future, io, net::SocketAddr, ops::Deref,
    pin::Pin, sync::Mutex,
};

use http_body_util::{BodyExt, Full};
use hyper::{
    body::{self, Bytes},
    server::conn::http1,
    service::service_fn,
    Request, Response,
};
use hyper_util::rt::TokioIo;
use tokio::net::TcpListener;

#[must_use]
pub(crate) struct HyperFixtureHandle<C: Deref<Target = HyperFixtureCollection>> {
    path: String,
    collection: C,
}

impl<C: Deref<Target = HyperFixtureCollection>> Drop for HyperFixtureHandle<C> {
    fn drop(&mut self) {
        let failed_request = {
            let mut services = self.collection.fixtures.lock().unwrap();
            services
                .get_mut(&*self.path)
                .expect("fixture not found")
                .assertion_failed_request
                .take()
        };
        if let Some(req) = failed_request {
            panic!("assertion failed for request {}: {:?}", self.path, req);
        }
    }
}

// BoxedBody for supporting streaming/chunked responses
type BoxedBody = http_body_util::combinators::BoxBody<Bytes, hyper::Error>;

// New type allowing both Full<Bytes> and BoxedBody response types
// This makes existing tests compatible with the new changes
pub(crate) type FixtureAssertionResult = (ResponseWrapper, Result<(), Request<body::Incoming>>);

pub(crate) struct ResponseWrapper(Response<BoxedBody>);

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

type SharedCollection = Mutex<BTreeMap<String, HyperServiceFixture>>;
#[derive(Default)]
pub(crate) struct HyperFixtureCollection {
    fixtures: SharedCollection,
}

impl HyperFixtureCollection {
    pub(crate) const fn new() -> Self {
        Self {
            fixtures: Mutex::new(BTreeMap::new()),
        }
    }
}

pub(crate) fn add_hyper_fixture<C, Fut, Resp>(
    collection: C,
    path: impl Into<String>,
    svc_fn: impl Fn(Request<body::Incoming>) -> Fut + Send + Sync + 'static,
) -> HyperFixtureHandle<C>
where
    C: Deref<Target = HyperFixtureCollection>,
    Fut: Future<Output = (Resp, Result<(), Request<body::Incoming>>)> + Send + 'static,
    Resp: Into<ResponseWrapper>,
{
    let mut path: String = path.into();
    if !path.starts_with('/') && !path.is_empty() {
        path.insert(0, '/');
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
        let path = path.clone();
        let mut services = collection.fixtures.lock().unwrap();
        services.insert(path, fixture);
    }
    HyperFixtureHandle { path, collection }
}

async fn handle_service(
    collection: impl Deref<Target = HyperFixtureCollection>,
    req: Request<body::Incoming>,
) -> Result<Response<BoxedBody>, Infallible> {
    let path = req.uri().path().to_owned();
    let fut = {
        let services = collection.fixtures.lock().unwrap();
        let fixture = services.get(&*path).unwrap();
        (fixture.svc)(req)
    };
    let (response, result) = fut.await;

    if let Err(req) = result {
        let mut services = collection.fixtures.lock().unwrap();
        let fixture = services.get_mut(&*path).unwrap();
        fixture.assertion_failed_request = Some(req);
    }

    Ok(response.0)
}

pub(crate) async fn spawn_service(
    collection: impl Deref<Target = HyperFixtureCollection> + Clone + Send + 'static,
) -> Result<u16, io::Error> {
    // TODO: handle panic
    let addr = SocketAddr::from(([127, 0, 0, 1], 0));

    let listener = TcpListener::bind(addr).await?;
    let port = listener.local_addr()?.port();

    tokio::spawn(async move {
        loop {
            let (stream, _) = listener.accept().await.expect("accept failed");
            let io = TokioIo::new(stream);
            let collection = collection.clone();

            tokio::task::spawn(async move {
                if let Err(err) = http1::Builder::new()
                    // `service_fn` converts our function in a `Service`
                    .serve_connection(
                        io,
                        service_fn(move |req| handle_service(collection.clone(), req)),
                    )
                    .with_upgrades()
                    .await
                {
                    eprintln!("Error serving connection: {err:?}");
                }
            });
        }
    });

    Ok(port)
}
