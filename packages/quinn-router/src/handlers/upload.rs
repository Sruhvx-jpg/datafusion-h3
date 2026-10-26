use http::{header, StatusCode};
use object_store::aws::AmazonS3Builder;
use object_store::buffered::BufWriter;
use object_store::path::Path as ObjectPath;
use object_store::ObjectStore;
use quinn_macros::put;
use quinn_router::{HttpRequest, HttpResponse};
use serde::Serialize;
use std::collections::HashSet;
use std::sync::{Arc, OnceLock};
use tokio::io::AsyncWriteExt;

static S3_STORE: OnceLock<Option<Arc<dyn ObjectStore>>> = OnceLock::new();

fn get_s3_store() -> Option<Arc<dyn ObjectStore>> {
    S3_STORE
        .get_or_init(|| {
            let endpoint = std::env::var("S3_ENDPOINT")
                .unwrap_or_else(|_| "http://127.0.0.1:9000".to_string());
            let bucket = std::env::var("S3_BUCKET")
                .unwrap_or_else(|_| "service4agent".to_string());
            let access_key = std::env::var("AWS_ACCESS_KEY_ID")
                .unwrap_or_else(|_| "minioadmin".to_string());
            let secret_key = std::env::var("AWS_SECRET_ACCESS_KEY")
                .unwrap_or_else(|_| "minioadmin".to_string());

            AmazonS3Builder::new()
                .with_endpoint(endpoint)
                .with_bucket_name(bucket)
                .with_region("us-east-1")
                .with_allow_http(true)
                .with_access_key_id(access_key)
                .with_secret_access_key(secret_key)
                .build()
                .ok()
                .map(|s| Arc::new(s) as Arc<dyn ObjectStore>)
        })
        .clone()
}

pub fn validate_filename(filename: &str) -> bool {
    if !filename.ends_with(".csv") || filename == ".csv" {
        return false;
    }

    if filename.contains('/')
        || filename.contains('\\')
        || filename.contains("..")
        || filename.chars().any(|c| c.is_control())
        || std::path::Path::new(filename).file_name() != Some(std::ffi::OsStr::new(filename))
    {
        return false;
    }

    true
}

#[derive(Debug, Serialize)]
pub struct UploadMetadata {
    pub status: String,
    pub filename: String,
    pub file_path: String,
    pub total_bytes: u64,
    pub total_chunks: usize,
    pub duplicate_chunks: usize,
    pub blake3_hash: String,
    pub storage: String,
}

/// Leaf Handler: Streaming PUT CSV upload using battle-tested BufWriter
#[put("/tables/{filename}")]
pub async fn upload_table(req: &mut HttpRequest) -> HttpResponse {
    // 1. Filename validation
    let filename = match req.path_param("filename") {
        Some(f) if !f.is_empty() => f.to_string(),
        _ => {
            let path = req.uri().path();
            path.rsplit('/').next().unwrap_or("").to_string()
        }
    };

    if !validate_filename(&filename) {
        return HttpResponse::BadRequest().json(serde_json::json!({
            "error": "Only .csv files are supported",
            "status": 400
        }));
    }

    // 2. Content-Type validation
    if let Some(ct_header) = req.headers().get(header::CONTENT_TYPE) {
        if let Ok(ct_str) = ct_header.to_str() {
            let media_type = ct_str.split(';').next().unwrap_or("").trim().to_lowercase();
            if media_type != "text/csv"
                && media_type != "application/csv"
                && media_type != "application/octet-stream"
            {
                return HttpResponse::build(StatusCode::UNSUPPORTED_MEDIA_TYPE).json(serde_json::json!({
                    "error": "Unsupported media type. Supported types are: text/csv, application/csv, application/octet-stream",
                    "status": 415
                }));
            }
        } else {
            return HttpResponse::build(StatusCode::UNSUPPORTED_MEDIA_TYPE).json(serde_json::json!({
                "error": "Invalid Content-Type header encoding",
                "status": 415
            }));
        }
    }

    // 3. Location and Crate-Provided BufWriter Setup
    let location = match ObjectPath::parse(format!("tables/{}", filename)) {
        Ok(p) => p,
        Err(e) => {
            return HttpResponse::BadRequest().json(serde_json::json!({
                "error": format!("Invalid storage path: {e}"),
                "status": 400
            }));
        }
    };

    let store = match get_s3_store() {
        Some(s) => s,
        None => {
            return HttpResponse::InternalServerError().json(serde_json::json!({
                "error": "S3 storage provider unavailable",
                "status": 500
            }));
        }
    };

    // Use battle-tested BufWriter from object_store crate (handles 5MB multipart chunks, zero-byte puts, abort on drop)
    let mut writer = BufWriter::new(store, location);

    let mut running_hasher = blake3::Hasher::new();
    let mut total_bytes: u64 = 0;
    let mut total_chunks: usize = 0;
    let mut duplicate_chunks: usize = 0;
    let mut seen_chunks: HashSet<[u8; 32]> = HashSet::new();

    const MAX_UPLOAD_SIZE: u64 = 10 * 1024 * 1024 * 1024; // 10 GiB

    // 4. Stream Loop: read chunks from QUIC stream and pipe to crate's BufWriter
    loop {
        let chunk_opt = match req.recv_chunk().await {
            Ok(c) => c,
            Err(e) => {
                return HttpResponse::BadRequest().json(serde_json::json!({
                    "error": format!("Stream read error: {e}"),
                    "status": 400
                }));
            }
        };

        let chunk = match chunk_opt {
            Some(c) => c,
            None => break, // EOF reached
        };

        if chunk.is_empty() {
            continue;
        }

        total_bytes += chunk.len() as u64;
        if total_bytes > MAX_UPLOAD_SIZE {
            return HttpResponse::build(StatusCode::PAYLOAD_TOO_LARGE).json(serde_json::json!({
                "error": "Upload exceeds 10 GiB maximum limit",
                "status": 413
            }));
        }

        total_chunks += 1;
        running_hasher.update(&chunk);

        let chunk_hash: [u8; 32] = *blake3::hash(&chunk).as_bytes();
        if !seen_chunks.insert(chunk_hash) {
            duplicate_chunks += 1;
        }

        // Pipe chunk directly to battle-tested S3 writer (AsyncWrite)
        if let Err(e) = writer.write_all(&chunk).await {
            tracing::error!("S3 write error: {e}");
            return HttpResponse::InternalServerError().json(serde_json::json!({
                "error": format!("S3 write failed: {e}"),
                "status": 500
            }));
        }
    }

    // 5. Finalize S3 upload via crate's shutdown()
    if let Err(e) = writer.shutdown().await {
        tracing::error!("S3 completion error: {e}");
        return HttpResponse::InternalServerError().json(serde_json::json!({
            "error": format!("S3 multipart completion failed: {e}"),
            "status": 500
        }));
    }

    let blake3_hash = running_hasher.finalize().to_hex().to_string();
    let bucket = std::env::var("S3_BUCKET").unwrap_or_else(|_| "service4agent".to_string());
    let file_path = format!("s3://{}/tables/{}", bucket, filename);

    let meta = UploadMetadata {
        status: "success".to_string(),
        filename,
        file_path,
        total_bytes,
        total_chunks,
        duplicate_chunks,
        blake3_hash,
        storage: "s3".to_string(),
    };

    HttpResponse::Ok().json(meta)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_filename_valid() {
        assert!(validate_filename("clean_test.csv"));
        assert!(validate_filename("table1_2024.csv"));
        assert!(validate_filename("my-data.csv"));
    }

    #[test]
    fn test_validate_filename_path_traversal() {
        assert!(!validate_filename("../clean_test.csv"));
        assert!(!validate_filename("..\\clean_test.csv"));
        assert!(!validate_filename("/clean_test.csv"));
        assert!(!validate_filename("dir/clean_test.csv"));
        assert!(!validate_filename("dir\\clean_test.csv"));
        assert!(!validate_filename("....csv"));
        assert!(!validate_filename(".csv"));
        assert!(!validate_filename("clean_test.csv\0"));
        assert!(!validate_filename("clean_test.csv\n"));
        assert!(!validate_filename("clean_test.txt"));
    }

    #[test]
    fn test_object_path_parse() {
        assert!(ObjectPath::parse("tables/valid.csv").is_ok());
        assert!(ObjectPath::parse("tables/../invalid.csv").is_err());
    }
}
