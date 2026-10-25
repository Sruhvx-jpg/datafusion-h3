use std::sync::Arc;
use actix_router::{Path, Router as ActixRouter};
use bytes::Bytes;
use http::{Request, Response, StatusCode};
use crate::internal::route::HandlerEntry;
use crate::internal::types::H3Stream;

/// Central compiled HTTP/3 Router (Runtime Dispatcher)
#[derive(Clone)]
pub struct Router {
    actix_router: Arc<ActixRouter<HandlerEntry>>,
}

impl Router {
    pub(crate) fn from_actix(actix_router: Arc<ActixRouter<HandlerEntry>>) -> Self {
        Self { actix_router }
    }

    /// Dispatches incoming HTTP/3 stream using Actix path-matching engine
    pub async fn dispatch(&self, req: Request<()>, mut stream: H3Stream) {
        let path_str = req.uri().path().to_string();
        let method = req.method().clone();
        let mut path = Path::new(path_str.clone());

        // 1. Actix-router Trie recognition
        if let Some((handler, _id)) = self.actix_router.recognize(&mut path) {
            if handler.method == method {
                if let Err(err) = (handler.func)(req, stream).await {
                    tracing::error!(path = %path_str, "Handler execution failed: {err}");
                }
                return;
            }
        }

        // 2. No route matched: Return 404 Not Found
        tracing::warn!(method = %method, path = %path_str, "Route not found");
        let resp = Response::builder()
            .status(StatusCode::NOT_FOUND)
            .header("content-type", "application/json")
            .body(())
            .unwrap();

        if let Ok(_) = stream.send_response(resp).await {
            let _ = stream.send_data(Bytes::from(r#"{"error":"Not Found"}"#)).await;
            let _ = stream.finish().await;
        }
    }
}
