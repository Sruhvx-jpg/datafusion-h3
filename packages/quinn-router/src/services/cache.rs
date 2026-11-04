use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

static REDIS_CLIENT: OnceLock<redis::Client> = OnceLock::new();

fn get_redis_client() -> Result<&'static redis::Client, Box<dyn std::error::Error + Send + Sync>> {
    if let Some(client) = REDIS_CLIENT.get() {
        return Ok(client);
    }

    let redis_url = std::env::var("REDIS_URL")
        .map_err(|_| "❌ [FATAL CONFIG] Missing required env variable 'REDIS_URL'")?;

    let client = redis::Client::open(redis_url)?;
    let _ = REDIS_CLIENT.set(client);

    Ok(REDIS_CLIENT.get().expect("Redis client initialized"))
}

/// Retrieve and deserialize cached JSON from Upstash Redis
pub async fn get_cached_json<T: for<'de> Deserialize<'de>>(key: &str) -> Option<T> {
    let client = get_redis_client().ok()?;
    let mut conn = client.get_multiplexed_async_connection().await.ok()?;

    let payload: Option<String> = conn.get(key).await.ok()?;
    payload.and_then(|raw| serde_json::from_str(&raw).ok())
}

/// Serialize and store JSON in Upstash Redis with TTL in seconds
pub async fn set_cached_json<T: Serialize>(key: &str, value: &T, ttl_seconds: u64) {
    let Ok(client) = get_redis_client() else {
        return;
    };
    let Ok(mut conn) = client.get_multiplexed_async_connection().await else {
        return;
    };
    let Ok(serialized) = serde_json::to_string(value) else {
        return;
    };

    let _: Result<(), _> = conn.set_ex(key, serialized, ttl_seconds).await;
}
