use std::future::Future;
use std::sync::Arc;
use actix_router::ResourceDef;
use http::{Method, Request};
use crate::internal::config::{HttpServiceFactory, ServiceConfig};
use crate::internal::types::{H3Stream, HandlerFn};

#[derive(Clone)]
pub struct HandlerEntry {
    pub method: Method,
    pub func: HandlerFn,
}

/// A route binding a path, HTTP method guard, and handler function
pub struct Route {
    path: String,
    entry: HandlerEntry,
}

impl HttpServiceFactory for Route {
    fn register(self, config: &mut ServiceConfig) {
        let subpath = self.path.trim_matches('/');
        let full_pattern = if subpath.is_empty() {
            config.prefix.clone()
        } else {
            format!("{}/{}", config.prefix.trim_end_matches('/'), subpath)
        };
        let rdef = ResourceDef::new(full_pattern);
        config.routes.push((rdef, self.entry));
    }
}

// Macro to generate all standard HTTP method constructors
macro_rules! define_method {
    ($fn_name:ident, $method:ident) => {
        pub fn $fn_name<F, Fut>(path: &str, f: F) -> Route
        where
            F: Fn(Request<()>, H3Stream) -> Fut + Send + Sync + 'static,
            Fut: Future<Output = Result<(), Box<dyn std::error::Error + Send + Sync>>> + Send + 'static,
        {
            let handler: HandlerFn = Arc::new(move |req, stream| Box::pin(f(req, stream)));
            Route {
                path: path.to_string(),
                entry: HandlerEntry {
                    method: Method::$method,
                    func: handler,
                },
            }
        }
    };
}

define_method!(get, GET);
define_method!(post, POST);
define_method!(put, PUT);
define_method!(delete, DELETE);
define_method!(patch, PATCH);
define_method!(head, HEAD);
define_method!(options, OPTIONS);
