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
    pub(crate) body_buffer: Option<Bytes>,
}

impl HttpRequest {
    pub fn new(req: Request<()>, stream: H3Stream) -> Self {
        Self {
            req,
            stream,
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

    /// Reads raw body bytes from the underlying HTTP/3 stream
    pub async fn read_body(&mut self) -> Result<Bytes, ApiError> {
        if let Some(ref b) = self.body_buffer {
            return Ok(b.clone());
        }

        let mut body = BytesMut::new();
        while let Ok(Some(mut chunk)) = self.stream.recv_data().await {
            while chunk.has_remaining() {
                let bytes = chunk.chunk();
                body.extend_from_slice(bytes);
                chunk.advance(bytes.len());
            }
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
