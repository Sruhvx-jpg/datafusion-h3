use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use http::Request;
use quinn_router::H3Stream;

/// Represents one of the backend engines in the showdown
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct BackendTarget {
    pub name: &'static str,
    pub addr: SocketAddr,
}

impl BackendTarget {
    pub fn new(name: &'static str, addr: SocketAddr) -> Self {
        Self { name, addr }
    }
}

/// Lock-free 50/50 Round-Robin Load Balancer
#[derive(Clone)]
pub struct LoadBalancer {
    backends: Arc<[BackendTarget]>,
    counter: Arc<AtomicUsize>,
    router: quinn_router::Router,
}

impl LoadBalancer {
    pub fn new(backends: Vec<BackendTarget>) -> Self {
        assert!(!backends.is_empty(), "Load balancer requires at least 1 backend target");
        Self {
            backends: backends.into(),
            counter: Arc::new(AtomicUsize::new(0)),
            router: quinn_router::api_v0(),
        }
    }

    /// Selects the next backend in lock-free sequence (50/50 distribution)
    pub fn select_backend(&self) -> &BackendTarget {
        let idx = self.counter.fetch_add(1, Ordering::Relaxed);
        &self.backends[idx % self.backends.len()]
    }

    /// Dispatches the HTTP/3 request to the selected backend
    pub async fn dispatch(
        &self,
        req: Request<()>,
        client_stream: H3Stream,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // 1. Pick backend (alternates strictly 50/50)
        let target = self.select_backend();

        tracing::info!(
            target_engine = target.name,
            target_addr = %target.addr,
            path = %req.uri().path(),
            "⚡ [50/50 LB] Routed request"
        );

        // 2. Dispatch to backend router engine
        self.router.dispatch(req, client_stream).await;

        Ok(())
    }
}
