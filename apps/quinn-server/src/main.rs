use std::net::SocketAddr;
use quinn::Endpoint;
use quinn_router::api_v0;
use server_conf::{create_server_config, ServerEnv};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Initialize Non-Blocking Tracing
    let (non_blocking_writer, _guard) = tracing_appender::non_blocking(std::io::stdout());
    tracing_subscriber::fmt()
        .with_writer(non_blocking_writer)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    // 2. Parse configuration & build QUIC config
    let env = ServerEnv::parse();
    let addr: SocketAddr = format!("{}:{}", env.host, env.port).parse()?;
    let server_config = create_server_config(&env)?;
    let endpoint = Endpoint::server(server_config, addr)?;

    // 3. Mount the central /api/v0 router directly from quinn_router
    let router = api_v0();

    tracing::info!(target: "server", "🚀 [Service4Agent] Running on UDP https://{} (HTTP/3)", addr);

    // 4. Connection Loop
    while let Some(incoming) = endpoint.accept().await {
        let router = router.clone();

        tokio::spawn(async move {
            let conn = match incoming.await {
                Ok(conn) => conn,
                Err(err) => {
                    tracing::warn!("QUIC handshake failed: {err}");
                    return;
                }
            };

            let mut h3_conn = match h3::server::builder()
                .build(h3_quinn::Connection::new(conn))
                .await
            {
                Ok(h3_conn) => h3_conn,
                Err(err) => {
                    tracing::warn!("HTTP/3 handshake failed: {err}");
                    return;
                }
            };

            // 5. Stream Loop: Delegate every request to our router
            while let Ok(Some(resolver)) = h3_conn.accept().await {
                let router = router.clone();

                tokio::spawn(async move {
                    let (req, stream) = match resolver.resolve_request().await {
                        Ok(pair) => pair,
                        Err(err) => {
                            tracing::warn!("Failed to resolve HTTP/3 request: {err}");
                            return;
                        }
                    };

                    tracing::info!(method = %req.method(), path = %req.uri().path(), "Request received");
                    
                    // Dispatch to /api/v0 router
                    router.dispatch(req, stream).await;
                });
            }
        });
    }

    Ok(())
}
