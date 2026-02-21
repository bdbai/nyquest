pub(crate) use writer::StreamWriter;

cfg_if::cfg_if! {
    if #[cfg(any(feature = "async-stream", feature = "blocking-stream"))] {
        mod ns_stream;
        mod writer;

        pub(crate) use ns_stream::InputStream;

        #[cfg(target_os = "macos")]
        const STREAM_BUFFER_SIZE: usize = 1024 * 32;
        #[cfg(not(target_os = "macos"))]
        const STREAM_BUFFER_SIZE: usize = 1024 * 8;
    } else {
        #[path = "stream/dummy_writer.rs"]
        mod writer;
    }
}

pub enum DataOrStream<S> {
    Data(Vec<u8>),
    Stream(S),
}
