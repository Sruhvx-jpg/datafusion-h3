use actix_router::ResourceDef;
use crate::internal::config::{HttpServiceFactory, ServiceConfig};

/// Scope isolates path prefixes without ripple effects (e.g. "/api/v0")
pub struct Scope {
    config: ServiceConfig,
}

impl Scope {
    pub fn new(prefix: &str) -> Self {
        let clean = format!("/{}", prefix.trim_matches('/'));
        Self {
            config: ServiceConfig::new(&clean),
        }
    }

    pub fn configure<F>(mut self, f: F) -> Self
    where
        F: FnOnce(&mut ServiceConfig),
    {
        self.config.configure(f);
        self
    }

    pub fn service<S>(mut self, service: S) -> Self
    where
        S: HttpServiceFactory,
    {
        self.config.service(service);
        self
    }
}

impl HttpServiceFactory for Scope {
    fn register(self, parent: &mut ServiceConfig) {
        let parent_prefix = parent.prefix.trim_end_matches('/');
        for (rdef, entry) in self.config.routes {
            let combined = format!("{}{}", parent_prefix, rdef.pattern().unwrap_or(""));
            parent.routes.push((ResourceDef::new(combined), entry));
        }
    }
}

/// Convenience web namespace (like actix_web::web)
pub mod web {
    use super::*;

    pub fn scope(prefix: &str) -> Scope {
        Scope::new(prefix)
    }
}
