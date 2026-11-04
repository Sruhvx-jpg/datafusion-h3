use crate::data_ops::extract_column_json_values;
use crate::internal::error::ApiError;
use datafusion::prelude::DataFrame;
use serde::Serialize;
use serde_json::Value as JsonValue;

/// Single contracted input object for unique value extraction
#[non_exhaustive]
pub struct UniqueValTypes {
    pub df: DataFrame,
    pub dataset_id: String,
    pub column: String,
    pub limit: Option<usize>,
}

impl UniqueValTypes {
    pub fn new(df: DataFrame, dataset_id: impl Into<String>, column: impl Into<String>) -> Self {
        Self {
            df,
            dataset_id: dataset_id.into(),
            column: column.into(),
            limit: None,
        }
    }

    pub fn with_limit(mut self, limit: usize) -> Self {
        self.limit = Some(limit);
        self
    }
}

/// Contracted response type
#[non_exhaustive]
#[derive(Debug, Clone, Serialize)]
pub struct UniqueValuesResponse {
    pub dataset_id: String,
    pub column: String,
    pub total_unique: usize,
    pub values: Vec<JsonValue>,
}

/// Takes strictly the single contracted object and executes the distinct query
pub async fn extract_unique_values(
    input: UniqueValTypes,
) -> Result<UniqueValuesResponse, ApiError> {
    let UniqueValTypes {
        df,
        dataset_id,
        column,
        limit,
    } = input;

    // 1. Validate column existence against schema (case-insensitive lookup)
    let schema = df.schema();
    let field = schema
        .fields()
        .iter()
        .find(|f| f.name().eq_ignore_ascii_case(&column))
        .ok_or_else(|| {
            ApiError::bad_request(format!("Column '{}' not found in dataset schema", column))
        })?;

    let canonical_col_name = field.name().clone();

    // 2. Build DataFusion projection + distinct query
    let mut distinct_df = df
        .select_columns(&[&canonical_col_name])
        .map_err(|e| ApiError::internal(format!("Projection failed: {e}")))?
        .distinct()
        .map_err(|e| ApiError::internal(format!("Distinct execution failed: {e}")))?;

    if let Some(l) = limit {
        distinct_df = distinct_df
            .limit(0, Some(l))
            .map_err(|e| ApiError::internal(format!("Limit failed: {e}")))?;
    }

    // 3. Execute and collect batches
    let batches = distinct_df
        .collect()
        .await
        .map_err(|e| ApiError::internal(format!("Query failed: {e}")))?;

    // 4. Extract distinct scalar values using shared helper
    let values = extract_column_json_values(&batches, 0);
    let total_unique = values.len();

    Ok(UniqueValuesResponse {
        dataset_id,
        column: canonical_col_name,
        total_unique,
        values,
    })
}
