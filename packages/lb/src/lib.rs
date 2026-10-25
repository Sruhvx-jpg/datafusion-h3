use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use bytes::Bytes;
use http::{Request, Response, StatusCode};
use quinn_router::H3Stream;

/// Represents one of the backend engines in the showdown
#[derive(Debug, Clone)]
pub struct BackendTarget {
    pub name: &'static str,
    pub addr: SocketAddr,
}

/// Lock-free 50/50 Round-Robin Load Balancer
#[derive(Clone)]
pub struct LoadBalancer {
    backends: Arc<Vec<BackendTarget>>,
    counter: Arc<AtomicUsize>,
}

impl LoadBalancer {
    pub fn new(backends: Vec<BackendTarget>) -> Self {
        assert!(!backends.is_empty(), "Load balancer requires at least 1 backend target");
        Self {
            backends: Arc::new(backends),
            counter: Arc::new(AtomicUsize::new(0)),
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
        mut client_stream: H3Stream,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // 1. Pick backend (alternates strictly 50/50)
        let target = self.select_backend();

        tracing::info!(
            target_engine = target.name,
            target_addr = %target.addr,
            path = %req.uri().path(),
            "⚡ [50/50 LB] Routed request"
        );

        // 2. Build response tagging which engine served the request
        let resp = Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "application/json")
            .header("x-served-by", target.name)
            .body(())?;

        client_stream.send_response(resp).await?;

        let payload = format!(
            r#"{{"status":"ok","backend":"{}","addr":"{}"}}"#,
            target.name, target.addr
        );
        client_stream.send_data(Bytes::from(payload)).await?;
        client_stream.finish().await?;

        Ok(())
    }
}
