use crate::services::cache::{get_cached_json, set_cached_json};
use crate::services::session::create_in_memory_context;
use crate::services::turso::get_dataset_by_id;
use datafusion::prelude::{CsvReadOptions, ParquetReadOptions};
use serde::{Deserialize, Serialize};

const SCHEMA_TTL_SECONDS: u64 = 86400; // 24-hour cache TTL

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaField {
    pub name: String,
    pub data_type: String, // Arrow type: "Utf8", "Int64", "Float64", "Timestamp"
    pub nullable: bool,
    pub is_numeric: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatasetSchemaResponse {
    pub dataset_id: String,
    pub dataset_name: String,
    pub version: i64,
    pub format: String,
    pub total_columns: usize,
    pub fields: Vec<SchemaField>,
}

/// Discovers dataset schema with Upstash Redis cache-aside
pub async fn inspect_dataset_schema(
    dataset_id: &str,
) -> Result<Option<DatasetSchemaResponse>, Box<dyn std::error::Error + Send + Sync>> {
    // 1. Fetch metadata (including version) from Turso
    let dataset = match get_dataset_by_id(dataset_id).await? {
        Some(d) => d,
        None => return Ok(None),
    };

    // 2. Cache Key: schema:{datasetId}:v{version}
    let cache_key = format!("schema:{}:v{}", dataset.id, dataset.version);

    // 3. Fast Path: Cache HIT (Sub-millisecond Upstash read)
    if let Some(cached_schema) = get_cached_json::<DatasetSchemaResponse>(&cache_key).await {
        return Ok(Some(cached_schema));
    }

    // 4. Slow Path: Cache MISS -> Hydrate DataFusion In-Memory
    let (ctx, memory_uri) = create_in_memory_context(&dataset.name, dataset.data).await?;

    let df = match dataset.format.to_lowercase().as_str() {
        "parquet" => {
            ctx.read_parquet(&memory_uri, ParquetReadOptions::default())
                .await?
        }
        _ => ctx.read_csv(&memory_uri, CsvReadOptions::default()).await?,
    };

    let schema = df.schema();
    let fields = schema
        .fields()
        .iter()
        .map(|f| SchemaField {
            name: f.name().clone(),
            data_type: f.data_type().to_string(),
            nullable: f.is_nullable(),
            is_numeric: f.data_type().is_numeric(),
        })
        .collect::<Vec<_>>();

    let response = DatasetSchemaResponse {
        dataset_id: dataset.id,
        dataset_name: dataset.name,
        version: dataset.version,
        format: dataset.format,
        total_columns: fields.len(),
        fields,
    };

    // 5. Store in Upstash Redis with 24h TTL
    set_cached_json(&cache_key, &response, SCHEMA_TTL_SECONDS).await;

    Ok(Some(response))
}
