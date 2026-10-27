extern crate self as quinn_router;

pub mod handlers;
pub mod internal;
pub mod router;

// Re-export public API cleanly
pub use handlers::upload::{S3Uri, TableName, UploadMetadata};
pub use internal::{
    app::App,
    config::{HttpServiceFactory, ServiceConfig},
    error::ApiError,
    request::HttpRequest,
    response::{HttpResponse, HttpResponseBuilder, IntoHttpResponse},
    route::{HandlerEntry, Route},
    scope::{web, Scope},
    service::Router,
    types::{BoxFuture, H3Stream, HandlerFn},
};

// Re-export Actix-style method macros directly at root
pub use quinn_macros::{delete, get, head, options, patch, post, put};

// Re-export central /api/v0 router
pub use router::api_v0;
