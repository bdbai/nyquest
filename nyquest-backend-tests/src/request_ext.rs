use hyper::{body, Request};

pub trait RequestExt {
    fn is_blocking(&self) -> bool;
}

impl RequestExt for Request<body::Incoming> {
    fn is_blocking(&self) -> bool {
        self.headers().get("blocking").map(|v| v.as_bytes()) == Some(b"1")
    }
}
