use std::sync::Arc;
use actix_router::{Path, Router as ActixRouter};
use bytes::Bytes;
use http::{Request, Response, StatusCode};
use crate::internal::request::HttpRequest;
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

    /// Dispatches incoming HTTP/3 stream using Actix path-matching engine.
    /// Passes HttpRequest to the handler and writes the response to the stream automatically.
    pub async fn dispatch(&self, req: Request<()>, stream: H3Stream) {
        let path_str = req.uri().path().to_string();
        let method = req.method().clone();
        let mut path = Path::new(path_str.clone());

        // 1. Actix-router Trie recognition
        if let Some((handler, _id)) = self.actix_router.recognize(&mut path) {
            if handler.method == method {
                let http_req = HttpRequest::new(req, stream, path);

                // Execute handler
                let (outcome, mut stream) = (handler.func)(http_req).await;

                match outcome {
                    Ok(response) => {
                        let (parts, body) = response.into_parts();
                        let head = Response::from_parts(parts, ());

                        if let Err(err) = stream.send_response(head).await {
                            tracing::error!("Failed to send response headers: {err}");
                            return;
                        }
                        if let Err(err) = stream.send_data(body).await {
                            tracing::error!("Failed to send response body: {err}");
                            return;
                        }
                        let _ = stream.finish().await;
                    }
                    Err(err) => {
                        tracing::error!(path = %path_str, "Handler execution failed: {err}");
                        let resp = Response::builder()
                            .status(StatusCode::INTERNAL_SERVER_ERROR)
                            .header("content-type", "application/json")
                            .body(())
                            .expect("static 500 response template is guaranteed valid");

                        if let Ok(_) = stream.send_response(resp).await {
                            let _ = stream.send_data(Bytes::from(r#"{"error":"Internal Server Error"}"#)).await;
                            let _ = stream.finish().await;
                        }
                    }
                }
                return;
            }
        }

        // 2. No route matched: Return 404 Not Found
        tracing::warn!(method = %method, path = %path_str, "Route not found");
        let mut stream = stream;
        let resp = Response::builder()
            .status(StatusCode::NOT_FOUND)
            .header("content-type", "application/json")
            .body(())
            .expect("static 404 response template is guaranteed valid");

        if let Ok(_) = stream.send_response(resp).await {
            let _ = stream.send_data(Bytes::from(r#"{"error":"Not Found"}"#)).await;
            let _ = stream.finish().await;
        }
    }
}
