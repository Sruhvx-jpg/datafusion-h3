use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use bytes::Bytes;
use http::Response;
use crate::internal::request::HttpRequest;

/// Type alias for the bidirectional HTTP/3 stream over Quinn
pub type H3Stream = h3::server::RequestStream<h3_quinn::BidiStream<Bytes>, Bytes>;

pub type HandlerResult = Result<Response<Bytes>, Box<dyn std::error::Error + Send + Sync>>;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Handler function signature: Takes HttpRequest and yields the response + stream for transmission
pub type HandlerFn = Arc<dyn Fn(HttpRequest) -> BoxFuture<'static, (HandlerResult, H3Stream)> + Send + Sync>;
