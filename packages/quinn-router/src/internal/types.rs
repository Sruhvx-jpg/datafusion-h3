use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use bytes::Bytes;
use http::Request;

/// Type alias for the bidirectional HTTP/3 stream over Quinn
pub type H3Stream = h3::server::RequestStream<h3_quinn::BidiStream<Bytes>, Bytes>;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub type HandlerFn = Arc<
    dyn Fn(Request<()>, H3Stream) -> BoxFuture<'static, Result<(), Box<dyn std::error::Error + Send + Sync>>>
        + Send
        + Sync,
>;
