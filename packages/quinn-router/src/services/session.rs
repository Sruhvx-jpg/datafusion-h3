use bytes::Bytes;
use datafusion::prelude::{SessionConfig, SessionContext};
use object_store::memory::InMemory;
use object_store::path::Path as ObjectPath;
use object_store::{ObjectStore, PutPayload};
use std::sync::Arc;
use url::Url;

/// Mounts raw file bytes into an in-memory object store and returns (SessionContext, memory_uri)
pub async fn create_in_memory_context(
    filename: &str,
    bytes: Vec<u8>,
) -> Result<(SessionContext, String), Box<dyn std::error::Error + Send + Sync>> {
    let in_memory_store = InMemory::new();
    let location = ObjectPath::parse(filename)?;

    in_memory_store
        .put(&location, PutPayload::from(Bytes::from(bytes)))
        .await?;

    let memory_url = Url::parse("memory://")?;
    let partitions = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(2)
        .max(2);

    let config = SessionConfig::new()
        .with_target_partitions(partitions)
        .with_batch_size(8192)
        .set_bool("datafusion.sql_parser.enable_ident_normalization", false);

    let ctx = SessionContext::new_with_config(config);
    ctx.register_object_store(&memory_url, Arc::new(in_memory_store));

    let dataset_uri = format!("memory:///{filename}");
    Ok((ctx, dataset_uri))
}
