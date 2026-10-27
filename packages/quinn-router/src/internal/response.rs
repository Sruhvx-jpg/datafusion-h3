use bytes::Bytes;
use http::{header, HeaderName, HeaderValue, Response, StatusCode};
use serde::Serialize;

/// Trait converting any handler output into an HTTP Response
pub trait IntoHttpResponse {
    fn into_http_response(self) -> Result<Response<Bytes>, Box<dyn std::error::Error + Send + Sync>>;
}

/// An HTTP response builder mimicking actix_web::HttpResponseBuilder
#[must_use = "an HttpResponseBuilder does nothing unless .body(), .json(), or .finish() is called"]
pub struct HttpResponseBuilder {
    status: StatusCode,
    headers: Vec<(HeaderName, HeaderValue)>,
}

impl HttpResponseBuilder {
    pub fn new(status: StatusCode) -> Self {
        Self {
            status,
            headers: Vec::new(),
        }
    }

    pub fn insert_header<K, V>(&mut self, key: K, value: V) -> &mut Self
    where
        HeaderName: TryFrom<K>,
        HeaderValue: TryFrom<V>,
    {
        if let (Ok(k), Ok(v)) = (HeaderName::try_from(key), HeaderValue::try_from(value)) {
            self.headers.push((k, v));
        }
        self
    }

    /// Serializes value to JSON, sets Content-Type: application/json, and returns HttpResponse
    pub fn json<T: Serialize>(&mut self, value: T) -> HttpResponse {
        let json_bytes = serde_json::to_vec(&value).unwrap_or_else(|_| b"{}".to_vec());
        self.insert_header(header::CONTENT_TYPE, "application/json");
        self.body(json_bytes)
    }

    /// Returns response with a byte or string body
    pub fn body<B: Into<Bytes>>(&mut self, body: B) -> HttpResponse {
        let mut builder = Response::builder().status(self.status);
        for (k, v) in &self.headers {
            builder = builder.header(k, v);
        }
        let resp = builder.body(body.into()).expect("static response builder invariants guaranteed valid");
        HttpResponse(resp)
    }

    /// Returns response with an empty body (e.g. 204 No Content, 404 Not Found)
    pub fn finish(&mut self) -> HttpResponse {
        self.body(Bytes::new())
    }
}

/// The response wrapper type mimicking actix_web::HttpResponse
#[derive(Debug)]
pub struct HttpResponse(pub Response<Bytes>);

impl HttpResponse {
    pub fn build(status: StatusCode) -> HttpResponseBuilder {
        HttpResponseBuilder::new(status)
    }

    // Static constructors matching Actix-Web
    #[allow(non_snake_case)]
    pub fn Ok() -> HttpResponseBuilder { HttpResponseBuilder::new(StatusCode::OK) }
    #[allow(non_snake_case)]
    pub fn Created() -> HttpResponseBuilder { HttpResponseBuilder::new(StatusCode::CREATED) }
    #[allow(non_snake_case)]
    pub fn Accepted() -> HttpResponseBuilder { HttpResponseBuilder::new(StatusCode::ACCEPTED) }
    #[allow(non_snake_case)]
    pub fn NoContent() -> HttpResponseBuilder { HttpResponseBuilder::new(StatusCode::NO_CONTENT) }
    #[allow(non_snake_case)]
    pub fn BadRequest() -> HttpResponseBuilder { HttpResponseBuilder::new(StatusCode::BAD_REQUEST) }
    #[allow(non_snake_case)]
    pub fn Unauthorized() -> HttpResponseBuilder { HttpResponseBuilder::new(StatusCode::UNAUTHORIZED) }
    #[allow(non_snake_case)]
    pub fn Forbidden() -> HttpResponseBuilder { HttpResponseBuilder::new(StatusCode::FORBIDDEN) }
    #[allow(non_snake_case)]
    pub fn NotFound() -> HttpResponseBuilder { HttpResponseBuilder::new(StatusCode::NOT_FOUND) }
    #[allow(non_snake_case)]
    pub fn UnsupportedMediaType() -> HttpResponseBuilder { HttpResponseBuilder::new(StatusCode::UNSUPPORTED_MEDIA_TYPE) }
    #[allow(non_snake_case)]
    pub fn InternalServerError() -> HttpResponseBuilder { HttpResponseBuilder::new(StatusCode::INTERNAL_SERVER_ERROR) }

    pub fn into_inner(self) -> Response<Bytes> {
        self.0
    }
}

impl IntoHttpResponse for HttpResponse {
    fn into_http_response(self) -> Result<Response<Bytes>, Box<dyn std::error::Error + Send + Sync>> {
        Ok(self.0)
    }
}

impl IntoHttpResponse for Response<Bytes> {
    fn into_http_response(self) -> Result<Response<Bytes>, Box<dyn std::error::Error + Send + Sync>> {
        Ok(self)
    }
}

impl<T, E> IntoHttpResponse for Result<T, E>
where
    T: IntoHttpResponse,
    E: std::error::Error + Send + Sync + 'static,
{
    fn into_http_response(self) -> Result<Response<Bytes>, Box<dyn std::error::Error + Send + Sync>> {
        match self {
            Ok(v) => v.into_http_response(),
            Err(e) => Err(Box::new(e)),
        }
    }
}
