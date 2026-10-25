use bytes::Bytes;
use http::{Request, Response, StatusCode};
use crate::H3Stream;
use quinn_macros::get;

/// Leaf Handler using pure Actix macro ergonomics: #[get("/health")]
#[get("/health")]
pub async fn health_check(
    _req: Request<()>,
    mut stream: H3Stream,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let resp = Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/json")
        .body(())?;

    stream.send_response(resp).await?;
    stream.send_data(Bytes::from(r#"{"status":"healthy","service":"service4agent"}"#)).await?;
    stream.finish().await?;
    Ok(())
}
