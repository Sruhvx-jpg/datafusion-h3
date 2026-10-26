use bytes::{Buf, Bytes, BytesMut};
use http::{HeaderMap, Method, Request, Uri};
use serde::de::DeserializeOwned;
use validator::Validate;
use crate::internal::error::ApiError;
use crate::internal::types::H3Stream;

/// HTTP request wrapper giving Express-style `req.json::<DTO>()` ergonomics
pub struct HttpRequest {
    pub(crate) req: Request<()>,
    pub(crate) stream: H3Stream,
    pub(crate) path: actix_router::Path<String>,
    pub(crate) body_buffer: Option<Bytes>,
}

impl HttpRequest {
    pub fn new(req: Request<()>, stream: H3Stream, path: actix_router::Path<String>) -> Self {
        Self {
            req,
            stream,
            path,
            body_buffer: None,
        }
    }

    pub fn method(&self) -> &Method {
        self.req.method()
    }

    pub fn uri(&self) -> &Uri {
        self.req.uri()
    }

    pub fn headers(&self) -> &HeaderMap {
        self.req.headers()
    }

    pub fn match_info(&self) -> &actix_router::Path<String> {
        &self.path
    }

    pub fn path_param(&self, name: &str) -> Option<&str> {
        self.path.get(name)
    }

    /// Receives the next data chunk from the HTTP/3 stream
    pub async fn recv_chunk(&mut self) -> Result<Option<Bytes>, ApiError> {
        match self.stream.recv_data().await {
            Ok(Some(mut chunk)) => {
                let bytes = chunk.copy_to_bytes(chunk.remaining());
                Ok(Some(bytes))
            }
            Ok(None) => Ok(None),
            Err(e) => Err(ApiError::internal(format!("Stream receive error: {e}"))),
        }
    }

    /// Reads raw body bytes from the underlying HTTP/3 stream
    pub async fn read_body(&mut self) -> Result<Bytes, ApiError> {
        if let Some(ref b) = self.body_buffer {
            return Ok(b.clone());
        }

        let mut body = BytesMut::new();
        while let Some(chunk) = self.recv_chunk().await? {
            body.extend_from_slice(&chunk);
        }

        let bytes = body.freeze();
        self.body_buffer = Some(bytes.clone());
        Ok(bytes)
    }

    /// Deserializes JSON and validates DTO rules automatically via `validator`
    pub async fn json<T>(&mut self) -> Result<T, ApiError>
    where
        T: DeserializeOwned + Validate,
    {
        let raw_bytes = self.read_body().await?;
        let dto: T = serde_json::from_slice(&raw_bytes).map_err(|e| {
            ApiError::bad_request(format!("Malformed JSON payload: {e}"))
        })?;

        // Automatic schema validation (e.g. email, min length, ranges)
        dto.validate().map_err(|e| {
            ApiError::bad_request(format!("Validation failed: {e}"))
        })?;

        Ok(dto)
    }

    /// Consumes HttpRequest and returns the underlying HTTP/3 stream for response writing
    pub fn into_stream(self) -> H3Stream {
        self.stream
    }
}
