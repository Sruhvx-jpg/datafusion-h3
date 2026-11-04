use libsql::Builder;
use serde::Serialize;
use std::sync::OnceLock;

#[derive(Debug, Clone, Serialize)]
pub struct DatasetSummary {
    pub id: String,
    pub name: String,
    pub format: String,
    pub row_count: Option<i64>,
    pub session_id: Option<String>,
    pub created_at: Option<i64>,
}

#[derive(Debug)]
pub struct DatasetRecord {
    pub id: String,
    pub name: String,
    pub format: String,
    pub version: i64,
    pub data: Vec<u8>,
}

static TURSO_DB: OnceLock<libsql::Database> = OnceLock::new();

async fn get_turso_db()
-> Result<&'static libsql::Database, Box<dyn std::error::Error + Send + Sync>> {
    if let Some(db) = TURSO_DB.get() {
        return Ok(db);
    }

    // Strict fail-fast: must be present in .env
    let url = std::env::var("DATABASE_URL")
        .map_err(|_| "❌ [FATAL CONFIG] Missing required env variable 'DATABASE_URL'")?;
    let token = std::env::var("TURSO_AUTH_TOKEN")
        .map_err(|_| "❌ [FATAL CONFIG] Missing required env variable 'TURSO_AUTH_TOKEN'")?;

    let db = Builder::new_remote(url, token).build().await?;
    let _ = TURSO_DB.set(db);

    Ok(TURSO_DB.get().expect("Turso DB initialized"))
}

pub async fn get_dataset_by_id(
    id: &str,
) -> Result<Option<DatasetRecord>, Box<dyn std::error::Error + Send + Sync>> {
    let db = get_turso_db().await?;
    let conn = db.connect()?;

    let mut rows = conn
        .query(
            "SELECT id, name, format, version, data FROM datasets WHERE id = ?1 LIMIT 1",
            [id],
        )
        .await?;

    if let Some(row) = rows.next().await? {
        let id: String = row.get(0)?;
        let name: String = row.get(1)?;
        let format: String = row.get(2)?;
        let version: i64 = row.get(3)?;
        let data: Vec<u8> = row.get(4)?;

        return Ok(Some(DatasetRecord {
            id,
            name,
            format,
            version,
            data,
        }));
    }

    Ok(None)
}

pub async fn list_datasets_by_user(
    user_id: &str,
) -> Result<Vec<DatasetSummary>, Box<dyn std::error::Error + Send + Sync>> {
    let db = get_turso_db().await?;
    let conn = db.connect()?;

    let mut rows = conn
        .query(
            "SELECT id, name, format, row_count, session_id, created_at FROM datasets WHERE user_id = ?1 ORDER BY created_at DESC",
            [user_id],
        )
        .await?;

    let mut list = Vec::new();
    while let Some(row) = rows.next().await? {
        list.push(DatasetSummary {
            id: row.get(0)?,
            name: row.get(1)?,
            format: row.get(2)?,
            row_count: row.get::<Option<i64>>(3)?,
            session_id: row.get::<Option<String>>(4)?,
            created_at: row.get::<Option<i64>>(5)?,
        });
    }

    Ok(list)
}
