#[cfg(any(feature = "async-stream", feature = "blocking-stream"))]
mod ns_stream;
#[cfg(any(feature = "async-stream", feature = "blocking-stream"))]
mod writer;
#[cfg(not(any(feature = "async-stream", feature = "blocking-stream")))]
#[path = "stream/dummy_writer.rs"]
mod writer;

#[cfg(any(feature = "async-stream", feature = "blocking-stream"))]
pub(crate) use ns_stream::InputStream;
pub(crate) use writer::StreamWriter;

pub enum DataOrStream<S> {
    Data(Vec<u8>),
    Stream(S),
}
