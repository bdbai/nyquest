#[derive(Debug, Clone)]
pub struct Request {
    pub relative_uri: String,
    pub method: String,
    // TODO: header, body
}

impl Request {
    pub fn new(relative_uri: String, method: String) -> Self {
        Self {
            relative_uri,
            method,
        }
    }
}
