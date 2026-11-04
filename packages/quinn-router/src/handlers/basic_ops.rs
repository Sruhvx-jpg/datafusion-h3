use crate::data_ops::basic::{UniqueValTypes, extract_unique_values};
use crate::services::session::create_in_memory_context;
use crate::services::turso::get_dataset_by_id;
use datafusion::prelude::{CsvReadOptions, ParquetReadOptions};
use http::StatusCode;
use quinn_macros::post;
use quinn_router::{HttpRequest, HttpResponse};
use serde::Deserialize;
use validator::Validate;

#[derive(Debug, Deserialize, Validate)]
pub struct UniqueValuesPayload {
    #[validate(length(min = 1, message = "Column name must not be empty"))]
    pub column: String,

    #[validate(range(min = 1, max = 10000, message = "Limit must be between 1 and 10000"))]
    pub limit: Option<usize>,
}

/// Leaf Handler: POST /api/v0/datasets/{id}/unique_values
#[post("/datasets/{id}/unique_values")]
pub async fn get_unique_values(req: &mut HttpRequest) -> HttpResponse {
    let dataset_id = match req.path_param("id") {
        Some(id) if !id.is_empty() => id.to_string(),
        _ => {
            return HttpResponse::BadRequest().json(serde_json::json!({
                "error": "Dataset ID path parameter is required",
                "status": 400
            }));
        }
    };

    let payload = match req.json::<UniqueValuesPayload>().await {
        Ok(p) => p,
        Err(err) => {
            return HttpResponse::BadRequest().json(serde_json::json!({
                "error": err.to_string(),
                "status": 400
            }));
        }
    };

    let dataset = match get_dataset_by_id(&dataset_id).await {
        Ok(Some(d)) => d,
        Ok(None) => {
            return HttpResponse::NotFound().json(serde_json::json!({
                "error": "Dataset not found",
                "status": 404
            }));
        }
        Err(err) => {
            return HttpResponse::build(StatusCode::INTERNAL_SERVER_ERROR).json(
                serde_json::json!({
                    "error": format!("Failed to retrieve dataset: {err}"),
                    "status": 500
                }),
            );
        }
    };

    let (ctx, memory_uri) = match create_in_memory_context(&dataset.name, dataset.data).await {
        Ok(pair) => pair,
        Err(err) => {
            return HttpResponse::build(StatusCode::INTERNAL_SERVER_ERROR).json(
                serde_json::json!({
                    "error": format!("In-memory context initialization failed: {err}"),
                    "status": 500
                }),
            );
        }
    };

    let df = match dataset.format.to_lowercase().as_str() {
        "parquet" => match ctx
            .read_parquet(&memory_uri, ParquetReadOptions::default())
            .await
        {
            Ok(df) => df,
            Err(e) => {
                return HttpResponse::InternalServerError()
                    .json(serde_json::json!({ "error": e.to_string() }));
            }
        },
        _ => match ctx.read_csv(&memory_uri, CsvReadOptions::default()).await {
            Ok(df) => df,
            Err(e) => {
                return HttpResponse::InternalServerError()
                    .json(serde_json::json!({ "error": e.to_string() }));
            }
        },
    };

    let mut input = UniqueValTypes::new(df, &dataset.id, payload.column);
    if let Some(l) = payload.limit {
        input = input.with_limit(l);
    }

    match extract_unique_values(input).await {
        Ok(response) => HttpResponse::Ok().json(response),
        Err(api_err) => HttpResponse::build(StatusCode::BAD_REQUEST).json(serde_json::json!({
            "error": api_err.to_string(),
            "status": 400
        })),
    }
}
