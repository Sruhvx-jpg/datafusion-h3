use actix_router::ResourceDef;
use http::Method;
use crate::internal::route::HandlerEntry;
use crate::internal::types::HandlerFn;

/// The core trait for pluggable services and scopes
pub trait HttpServiceFactory {
    fn register(self, config: &mut ServiceConfig);
}

/// The configuration context passed through `.configure()`
pub struct ServiceConfig {
    pub(crate) prefix: String,
    pub(crate) routes: Vec<(ResourceDef, HandlerEntry)>,
}

impl ServiceConfig {
    pub fn new(prefix: &str) -> Self {
        let clean = if prefix.is_empty() || prefix == "/" {
            "".to_string()
        } else {
            format!("/{}", prefix.trim_matches('/'))
        };
        Self {
            prefix: clean,
            routes: Vec::new(),
        }
    }

    /// Registers a route pattern, HTTP method, and handler function
    pub fn register_route(&mut self, path: &str, method: Method, handler: HandlerFn) {
        let subpath = path.trim_matches('/');
        let full_pattern = if subpath.is_empty() {
            self.prefix.clone()
        } else {
            format!("{}/{}", self.prefix.trim_end_matches('/'), subpath)
        };
        let rdef = ResourceDef::new(full_pattern);
        self.routes.push((rdef, HandlerEntry { method, func: handler }));
    }

    /// Plugs in a sub-configuration function without ripple effects
    pub fn configure<F>(&mut self, f: F) -> &mut Self
    where
        F: FnOnce(&mut ServiceConfig),
    {
        f(self);
        self
    }

    /// Plugs in any service or scope
    pub fn service<S>(&mut self, service: S) -> &mut Self
    where
        S: HttpServiceFactory,
    {
        service.register(self);
        self
    }
}
