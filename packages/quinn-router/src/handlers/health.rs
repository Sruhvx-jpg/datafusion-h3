use quinn_macros::get;
use quinn_router::{HttpRequest, HttpResponse};
use serde_json::json;

/// Leaf Handler: Pure Actix Ergonomics: req: &mut HttpRequest -> HttpResponse!
#[get("/health")]
pub async fn health_check(_req: &mut HttpRequest) -> HttpResponse {
    HttpResponse::Ok().json(json!({
        "status": "healthy",
        "service": "service4agent"
    }))
}
