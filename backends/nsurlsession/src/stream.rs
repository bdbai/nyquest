mod ns_stream;
mod writer;

pub(crate) use ns_stream::InputStream;
pub(crate) use writer::StreamWriter;

pub enum DataOrStream<S> {
    Data(Vec<u8>),
    Stream(S),
}
