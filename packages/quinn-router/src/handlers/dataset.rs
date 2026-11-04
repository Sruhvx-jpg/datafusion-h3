use crate::services::schema::inspect_dataset_schema;
use crate::services::turso::list_datasets_by_user;
use http::StatusCode;
use quinn_macros::get;
use quinn_router::{HttpRequest, HttpResponse};

/// Route 1: GET /api/v0/users/{user_id}/datasets
/// Returns metadata for all datasets owned by user (strictly excludes raw data BLOBs)
#[get("/users/{user_id}/datasets")]
pub async fn list_user_datasets(req: &mut HttpRequest) -> HttpResponse {
    let user_id = match req.path_param("user_id") {
        Some(id) if !id.is_empty() => id,
        _ => {
            return HttpResponse::BadRequest().json(serde_json::json!({
                "error": "user_id path parameter is required",
                "status": 400
            }));
        }
    };

    match list_datasets_by_user(user_id).await {
        Ok(datasets) => HttpResponse::Ok().json(datasets),
        Err(err) => {
            HttpResponse::build(StatusCode::INTERNAL_SERVER_ERROR).json(serde_json::json!({
                "error": format!("Failed to retrieve user datasets: {err}"),
                "status": 500
            }))
        }
    }
}

/// Route 2: GET /api/v0/datasets/{id}/schema
/// Discovers dataset Arrow schema (inferred via DataFusion or cached schema JSON)
#[get("/datasets/{id}/schema")]
pub async fn get_dataset_schema(req: &mut HttpRequest) -> HttpResponse {
    let dataset_id = match req.path_param("id") {
        Some(id) if !id.is_empty() => id,
        _ => {
            return HttpResponse::BadRequest().json(serde_json::json!({
                "error": "dataset id path parameter is required",
                "status": 400
            }));
        }
    };

    match inspect_dataset_schema(dataset_id).await {
        Ok(Some(schema)) => HttpResponse::Ok().json(schema),
        Ok(None) => HttpResponse::NotFound().json(serde_json::json!({
            "error": "Dataset not found",
            "status": 404
        })),
        Err(err) => {
            HttpResponse::build(StatusCode::INTERNAL_SERVER_ERROR).json(serde_json::json!({
                "error": format!("Schema extraction failed: {err}"),
                "status": 500
            }))
        }
    }
}
