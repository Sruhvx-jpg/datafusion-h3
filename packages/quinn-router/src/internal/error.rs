use bytes::Bytes;
use http::{Response, StatusCode};
use serde::Serialize;
use crate::internal::response::{HttpResponse, IntoHttpResponse};

/// Standardized API error matching industry production patterns
#[derive(Debug, Clone, Serialize)]
pub struct ApiError {
    #[serde(skip)]
    pub status: StatusCode,
    pub message: String,
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.status, self.message)
    }
}

impl std::error::Error for ApiError {}

impl ApiError {
    pub fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }

    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, message)
    }

    pub fn unauthorized(message: impl Into<String>) -> Self {
        Self::new(StatusCode::UNAUTHORIZED, message)
    }

    pub fn forbidden(message: impl Into<String>) -> Self {
        Self::new(StatusCode::FORBIDDEN, message)
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, message)
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, message)
    }
}

impl IntoHttpResponse for ApiError {
    fn into_http_response(self) -> Result<Response<Bytes>, Box<dyn std::error::Error + Send + Sync>> {
        let resp = HttpResponse::build(self.status).json(serde_json::json!({
            "error": self.message,
            "status": self.status.as_u16()
        }));
        Ok(resp.into_inner())
    }
}
