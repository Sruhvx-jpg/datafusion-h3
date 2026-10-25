use crate::internal::{web, App, ServiceConfig};
use crate::handlers::health::health_check;
use crate::Router;

/// Central entry point for the /api/v0 router.
/// Quinn-server only imports this function—nothing else!
pub fn api_v0() -> Router {
    App::new()
        .configure(mount_api_v0)
        .finish()
}

/// Mounts /api/v0 and attaches all child routes inside packages/quinn-router
pub fn mount_api_v0(cfg: &mut ServiceConfig) {
    cfg.service(
        web::scope("/api/v0")
            .service(health_check) // Plugs in #[get("/health")] macro service
            // Future child routes plug in right here:
            // .service(query_handler)
    );
}
