use std::collections::HashSet;
use std::sync::{Arc, OnceLock};
use bytes::BytesMut;
use http::{header, StatusCode};
use object_store::aws::AmazonS3Builder;
use object_store::path::Path as ObjectPath;
use object_store::{MultipartUpload, ObjectStore, PutPayload, PutResult, UploadPart};
use quinn_macros::put;
use quinn_router::{ApiError, HttpRequest, HttpResponse};
use serde::Serialize;
use tokio::io::AsyncWriteExt;

const MIN_MULTIPART_PART_SIZE: usize = 5 * 1024 * 1024; // 5 MiB (S3 minimum part size)
const MAX_BUFFER_CAPACITY: usize = 8 * 1024 * 1024;      // 8 MiB (RAM limit)
const MAX_UPLOAD_SIZE: u64 = 10 * 1024 * 1024 * 1024;   // 10 GiB (Max upload size limit)

static S3_STORE: OnceLock<Option<Arc<dyn ObjectStore>>> = OnceLock::new();

/// RAII Guard ensuring multipart uploads are aborted on error or premature drop
struct MultipartAbortGuard {
    upload: Option<Box<dyn MultipartUpload>>,
    completed: bool,
}

impl MultipartAbortGuard {
    fn new(upload: Box<dyn MultipartUpload>) -> Self {
        Self {
            upload: Some(upload),
            completed: false,
        }
    }

    fn put_part(&mut self, data: PutPayload) -> UploadPart {
        self.upload.as_mut().expect("multipart active").put_part(data)
    }

    async fn complete_upload(mut self) -> Result<PutResult, object_store::Error> {
        let mut upload = match self.upload.take() {
            Some(u) => u,
            None => {
                return Err(object_store::Error::Generic {
                    store: "s3",
                    source: "Multipart upload already finished or aborted".into(),
                });
            }
        };

        match upload.complete().await {
            Ok(res) => {
                self.completed = true;
                Ok(res)
            }
            Err(e) => {
                if let Err(abort_err) = upload.abort().await {
                    tracing::warn!(error = %abort_err, "Failed to abort S3 multipart upload");
                }
                self.completed = true;
                Err(e)
            }
        }
    }

    async fn abort_upload(mut self) -> Result<(), object_store::Error> {
        self.completed = true;
        if let Some(mut upload) = self.upload.take() {
            upload.abort().await
        } else {
            Ok(())
        }
    }
}

impl Drop for MultipartAbortGuard {
    fn drop(&mut self) {
        if !self.completed {
            if let Some(mut upload) = self.upload.take() {
                tokio::spawn(async move {
                    tracing::warn!("Aborting incomplete S3 multipart upload to prevent orphaned storage leaks");
                    if let Err(e) = upload.abort().await {
                        tracing::warn!(error = %e, "Failed to abort S3 multipart upload");
                    }
                });
            }
        }
    }
}

/// Response payload containing upload and fingerprinting metadata
#[derive(Debug, Serialize)]
pub struct UploadMetadata {
    pub status: String,
    pub filename: String,
    pub total_bytes: u64,
    pub total_chunks: usize,
    pub duplicate_chunks: usize,
    pub blake3_hash: String,
    pub storage: String,
}

fn create_s3_store() -> Result<Arc<dyn ObjectStore>, object_store::Error> {
    let endpoint = std::env::var("S3_ENDPOINT")
        .unwrap_or_else(|_| "http://127.0.0.1:9000".to_string());
    let bucket = std::env::var("S3_BUCKET")
        .unwrap_or_else(|_| "service4agent".to_string());
    let access_key = std::env::var("AWS_ACCESS_KEY_ID")
        .unwrap_or_else(|_| "minioadmin".to_string());
    let secret_key = std::env::var("AWS_SECRET_ACCESS_KEY")
        .unwrap_or_else(|_| "minioadmin".to_string());

    let s3 = AmazonS3Builder::new()
        .with_endpoint(endpoint)
        .with_bucket_name(bucket)
        .with_region("us-east-1")
        .with_allow_http(true)
        .with_access_key_id(access_key)
        .with_secret_access_key(secret_key)
        .build()?;

    Ok(Arc::new(s3))
}

fn get_s3_store() -> Option<Arc<dyn ObjectStore>> {
    S3_STORE
        .get_or_init(|| match create_s3_store() {
            Ok(store) => Some(store),
            Err(e) => {
                tracing::warn!("Failed to initialize S3 ObjectStore: {e}");
                None
            }
        })
        .clone()
}

fn is_auth_error(e: &object_store::Error) -> bool {
    matches!(
        e,
        object_store::Error::Unauthenticated { .. }
            | object_store::Error::PermissionDenied { .. }
    )
}

fn validate_filename(filename: &str) -> bool {
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

async fn open_local_file(filename: &str) -> Result<tokio::fs::File, ApiError> {
    let local_dir = std::env::var("LOCAL_STORAGE_DIR")
        .unwrap_or_else(|_| "/tmp/datafusion-h3/tables".to_string());
    tokio::fs::create_dir_all(&local_dir)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to create local storage dir: {e}")))?;
    let file_path = std::path::Path::new(&local_dir).join(filename);
    tokio::fs::File::create(&file_path)
        .await
        .map_err(|e| ApiError::internal(format!("Failed to create local file: {e}")))
}

/// Leaf Handler: Streaming PUT CSV upload with MinIO S3 integration and BLAKE3 fingerprinting
#[put("/tables/{filename}")]
pub async fn upload_table(req: &mut HttpRequest) -> HttpResponse {
    // 1. Filename validation: ONLY accept valid .csv filenames and prevent path traversal
    let filename = match req.path_param("filename") {
        Some(f) if !f.is_empty() => f.to_string(),
        _ => {
            let path = req.uri().path();
            path.rsplit('/').next().unwrap_or("").to_string()
        }
    };

    if !validate_filename(&filename) {
        return HttpResponse::BadRequest().json(serde_json::json!({
            "error": "Invalid filename: must be a valid .csv filename without path traversal or control characters",
            "status": 400
        }));
    }

    // 2. Content-Type validation: If provided, ensure it is text/csv, application/csv, or application/octet-stream
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

    // 3. Setup S3 & State Machine
    let s3_store = get_s3_store();
    let location = match ObjectPath::parse(format!("tables/{}", filename)) {
        Ok(loc) => loc,
        Err(e) => {
            return HttpResponse::BadRequest().json(serde_json::json!({
                "error": format!("Invalid object path: {e}"),
                "status": 400
            }));
        }
    };
    let mut fingerprints: HashSet<[u8; 32]> = HashSet::new();

    let mut running_hasher = blake3::Hasher::new();
    let mut total_bytes: u64 = 0;
    let mut total_chunks: usize = 0;
    let mut duplicate_chunks: usize = 0;

    let mut buffer = BytesMut::with_capacity(MAX_BUFFER_CAPACITY);

    enum StorageState {
        Buffering,
        S3Multipart(MultipartAbortGuard),
        LocalFile(tokio::fs::File),
    }

    let mut state = StorageState::Buffering;

    // 4. Stream Loop: read chunks, compute BLAKE3, buffer, and stream to storage
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
                "error": "Payload too large: upload exceeds 10 GiB limit",
                "status": 413
            }));
        }

        total_chunks += 1;
        running_hasher.update(&chunk);

        let chunk_hash: [u8; 32] = *blake3::hash(&chunk).as_bytes();
        if !fingerprints.insert(chunk_hash) {
            duplicate_chunks += 1;
        }

        match &mut state {
            StorageState::Buffering => {
                if buffer.len() + chunk.len() > MAX_BUFFER_CAPACITY {
                    return HttpResponse::build(StatusCode::PAYLOAD_TOO_LARGE).json(serde_json::json!({
                        "error": "Exceeded maximum buffer capacity",
                        "status": 413
                    }));
                }
                buffer.extend_from_slice(&chunk);
                if buffer.len() >= MIN_MULTIPART_PART_SIZE {
                    if let Some(store) = s3_store.as_ref() {
                        match store.put_multipart(&location).await {
                            Ok(upload) => {
                                let mut guard = MultipartAbortGuard::new(upload);
                                let part_bytes = buffer.split_to(buffer.len()).freeze();
                                if let Err(e) = guard.put_part(part_bytes.clone().into()).await {
                                    tracing::error!("Failed to upload initial part to S3: {e}");
                                    let _ = guard.abort_upload().await;
                                    let mut file = match open_local_file(&filename).await {
                                        Ok(f) => f,
                                        Err(err) => return HttpResponse::InternalServerError().json(serde_json::json!({ "error": err.to_string(), "status": 500 })),
                                    };
                                    if let Err(e) = file.write_all(&part_bytes).await {
                                        return HttpResponse::InternalServerError().json(serde_json::json!({ "error": e.to_string(), "status": 500 }));
                                    }
                                    state = StorageState::LocalFile(file);
                                } else {
                                    state = StorageState::S3Multipart(guard);
                                }
                            }
                            Err(e) => {
                                if is_auth_error(&e) {
                                    return HttpResponse::InternalServerError().json(serde_json::json!({
                                        "error": format!("S3 authentication failed: {e}"),
                                        "status": 500
                                    }));
                                }
                                tracing::warn!("MinIO S3 unreachable ({e}), falling back to local storage");
                                let mut file = match open_local_file(&filename).await {
                                    Ok(f) => f,
                                    Err(err) => return HttpResponse::InternalServerError().json(serde_json::json!({ "error": err.to_string(), "status": 500 })),
                                };
                                let buffered_data = buffer.split_to(buffer.len()).freeze();
                                if let Err(e) = file.write_all(&buffered_data).await {
                                    return HttpResponse::InternalServerError().json(serde_json::json!({ "error": e.to_string(), "status": 500 }));
                                }
                                state = StorageState::LocalFile(file);
                            }
                        }
                    } else {
                        let mut file = match open_local_file(&filename).await {
                            Ok(f) => f,
                            Err(err) => return HttpResponse::InternalServerError().json(serde_json::json!({ "error": err.to_string(), "status": 500 })),
                        };
                        let buffered_data = buffer.split_to(buffer.len()).freeze();
                        if let Err(e) = file.write_all(&buffered_data).await {
                            return HttpResponse::InternalServerError().json(serde_json::json!({ "error": e.to_string(), "status": 500 }));
                        }
                        state = StorageState::LocalFile(file);
                    }
                }
            }
            StorageState::S3Multipart(guard) => {
                if buffer.len() + chunk.len() > MAX_BUFFER_CAPACITY {
                    return HttpResponse::build(StatusCode::PAYLOAD_TOO_LARGE).json(serde_json::json!({
                        "error": "Exceeded maximum buffer capacity",
                        "status": 413
                    }));
                }
                buffer.extend_from_slice(&chunk);
                if buffer.len() >= MIN_MULTIPART_PART_SIZE {
                    let part_bytes = buffer.split_to(buffer.len()).freeze();
                    if let Err(e) = guard.put_part(part_bytes.into()).await {
                        tracing::error!("Failed to upload part to S3: {e}");
                        return HttpResponse::InternalServerError().json(serde_json::json!({
                            "error": format!("S3 multipart part upload failed: {e}"),
                            "status": 500
                        }));
                    }
                }
            }
            StorageState::LocalFile(file) => {
                if let Err(e) = file.write_all(&chunk).await {
                    return HttpResponse::InternalServerError().json(serde_json::json!({
                        "error": format!("Local file write failed: {e}"),
                        "status": 500
                    }));
                }
            }
        }
    }

    // 5. Finalize Storage
    let storage = match state {
        StorageState::Buffering => {
            let file_bytes = buffer.split_to(buffer.len()).freeze();
            let mut uploaded = false;

            if let Some(store) = s3_store.as_ref() {
                match store.put(&location, file_bytes.clone().into()).await {
                    Ok(_) => {
                        uploaded = true;
                    }
                    Err(e) => {
                        if is_auth_error(&e) {
                            return HttpResponse::InternalServerError().json(serde_json::json!({
                                "error": format!("S3 authentication failed: {e}"),
                                "status": 500
                            }));
                        }
                        tracing::warn!("MinIO S3 unreachable ({e}), falling back to local file storage");
                    }
                }
            }

            if uploaded {
                "s3".to_string()
            } else {
                let mut file = match open_local_file(&filename).await {
                    Ok(f) => f,
                    Err(err) => return HttpResponse::InternalServerError().json(serde_json::json!({ "error": err.to_string(), "status": 500 })),
                };
                if let Err(e) = file.write_all(&file_bytes).await {
                    return HttpResponse::InternalServerError().json(serde_json::json!({ "error": e.to_string(), "status": 500 }));
                }
                if let Err(e) = file.flush().await {
                    return HttpResponse::InternalServerError().json(serde_json::json!({ "error": e.to_string(), "status": 500 }));
                }
                "local".to_string()
            }
        }
        StorageState::S3Multipart(mut guard) => {
            if !buffer.is_empty() {
                let final_bytes = buffer.split_to(buffer.len()).freeze();
                if let Err(e) = guard.put_part(final_bytes.into()).await {
                    return HttpResponse::InternalServerError().json(serde_json::json!({
                        "error": format!("Failed to upload final multipart part: {e}"),
                        "status": 500
                    }));
                }
            }
            if let Err(e) = guard.complete_upload().await {
                return HttpResponse::InternalServerError().json(serde_json::json!({
                    "error": format!("Failed to complete S3 multipart upload: {e}"),
                    "status": 500
                }));
            }
            "s3".to_string()
        }
        StorageState::LocalFile(mut file) => {
            if let Err(e) = file.flush().await {
                return HttpResponse::InternalServerError().json(serde_json::json!({
                    "error": format!("Failed to flush local file: {e}"),
                    "status": 500
                }));
            }
            "local".to_string()
        }
    };

    let blake3_hash = running_hasher.finalize().to_hex().to_string();

    let meta = UploadMetadata {
        status: "success".to_string(),
        filename,
        total_bytes,
        total_chunks,
        duplicate_chunks,
        blake3_hash,
        storage,
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
    fn test_bytes_mut_split_to() {
        let mut buf = BytesMut::with_capacity(1024);
        buf.extend_from_slice(b"hello world");
        assert_eq!(buf.len(), 11);

        let extracted = buf.split_to(buf.len()).freeze();
        assert_eq!(&extracted[..], b"hello world");
        assert_eq!(buf.len(), 0);
    }

    #[test]
    fn test_object_path_parse() {
        let valid_path = ObjectPath::parse(format!("tables/{}", "clean_test.csv"));
        assert!(valid_path.is_ok());
    }

    #[test]
    fn test_scoped_fingerprints_deduplication() {
        let mut fingerprints: HashSet<[u8; 32]> = HashSet::new();
        let chunk1 = b"chunk 1";
        let chunk2 = b"chunk 2";
        let chunk1_dup = b"chunk 1";

        let hash1: [u8; 32] = *blake3::hash(chunk1).as_bytes();
        let hash2: [u8; 32] = *blake3::hash(chunk2).as_bytes();
        let hash1_dup: [u8; 32] = *blake3::hash(chunk1_dup).as_bytes();

        let mut duplicate_chunks = 0;
        if !fingerprints.insert(hash1) { duplicate_chunks += 1; }
        if !fingerprints.insert(hash2) { duplicate_chunks += 1; }
        if !fingerprints.insert(hash1_dup) { duplicate_chunks += 1; }

        assert_eq!(duplicate_chunks, 1);
        assert_eq!(fingerprints.len(), 2);
    }
}
