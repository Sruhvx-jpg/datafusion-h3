use std::sync::Arc;
use actix_router::Router as ActixRouter;
use crate::internal::config::{HttpServiceFactory, ServiceConfig};
use crate::internal::service::Router;

/// Top-level Application Builder (Identical to actix_web::App)
#[must_use = "an App does nothing until finished into a Router via .finish()"]
pub struct App {
    config: ServiceConfig,
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

impl App {
    pub fn new() -> Self {
        Self {
            config: ServiceConfig::new(""),
        }
    }

    #[must_use]
    pub fn configure<F>(mut self, f: F) -> Self
    where
        F: FnOnce(&mut ServiceConfig),
    {
        self.config.configure(f);
        self
    }

    #[must_use]
    pub fn service<S>(mut self, service: S) -> Self
    where
        S: HttpServiceFactory,
    {
        self.config.service(service);
        self
    }

    /// Builds the final compiled Router for the HTTP/3 server
    pub fn finish(self) -> Router {
        let mut builder = ActixRouter::build();
        for (rdef, entry) in self.config.routes {
            builder.push(rdef, entry, ());
        }
        Router::from_actix(Arc::new(builder.finish()))
    }
}
