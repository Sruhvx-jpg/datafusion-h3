use crate::Router;
use crate::handlers::basic_ops::get_unique_values;
use crate::handlers::dataset::{get_dataset_schema, list_user_datasets};
use crate::handlers::health::health_check;
use crate::handlers::upload::upload_table;
use crate::internal::{App, ServiceConfig, web};

/// Central entry point for the /api/v0 router.
/// Quinn-server only imports this function—nothing else!
pub fn api_v0() -> Router {
    App::new().configure(mount_api_v0).finish()
}

/// Mounts /api/v0 and attaches all child routes inside packages/quinn-router
pub fn mount_api_v0(cfg: &mut ServiceConfig) {
    cfg.service(
        web::scope("/api/v0")
            .service(health_check) // Plugs in #[get("/health")] macro service
            .service(upload_table) // Plugs in #[put("/tables/{filename}")] macro service
            .service(list_user_datasets) // GET /api/v0/users/{user_id}/datasets
            .service(get_dataset_schema) // GET /api/v0/datasets/{id}/schema
            .service(get_unique_values), // POST /api/v0/datasets/{id}/unique_values
    );
}
