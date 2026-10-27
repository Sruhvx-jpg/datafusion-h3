use std::net::SocketAddr;
use lb::{BackendTarget, LoadBalancer};
use quinn::Endpoint;
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

    // 2. Load environment & configure Gateway endpoint
    let env = ServerEnv::parse();
    // Gateway listens upfront on port 4430
    let gateway_addr: SocketAddr = format!("{}:4430", env.host).parse()?;
    let server_config = create_server_config(&env)?;
    let endpoint = Endpoint::server(server_config, gateway_addr)?;

    // 3. Initialize Load Balancer targeting the backends
    let lb = LoadBalancer::new(vec![
        BackendTarget::new(
            "quinn",
            format!("{}:{}", env.host, env.port).parse()?, // 127.0.0.1:4433
        ),
    ]);

    tracing::info!(target: "gateway", "🌐 [Gateway LB] Listening on UDP https://{}", gateway_addr);

    // 4. Accept client connections on :4430
    while let Some(incoming) = endpoint.accept().await {
        let lb = lb.clone();

        tokio::spawn(async move {
            let conn = match incoming.await {
                Ok(conn) => conn,
                Err(err) => {
                    match &err {
                        quinn::ConnectionError::ApplicationClosed(app) => {
                            tracing::warn!("Gateway ApplicationClosed: code=0x{:x} ({}), reason={:?}", app.error_code.into_inner(), app.error_code, app.reason);
                        }
                        _ => {
                            tracing::warn!("Client QUIC handshake failed: {err:?}");
                        }
                    }
                    return;
                }
            };

            let mut h3_conn = match h3::server::builder()
                .build(h3_quinn::Connection::new(conn))
                .await
            {
                Ok(h3_conn) => h3_conn,
                Err(err) => {
                    tracing::warn!("Client HTTP/3 handshake failed: {err}");
                    return;
                }
            };

            // 5. Accept client streams & dispatch via Load Balancer
            while let Ok(Some(resolver)) = h3_conn.accept().await {
                let lb = lb.clone();

                tokio::spawn(async move {
                    let (req, stream) = match resolver.resolve_request().await {
                        Ok(pair) => pair,
                        Err(err) => {
                            tracing::warn!("Failed to resolve client request: {err}");
                            return;
                        }
                    };

                    // Dispatch via Load Balancer
                    if let Err(err) = lb.dispatch(req, stream).await {
                        tracing::error!("Load balancer dispatch error: {err}");
                    }
                });
            }
        });
    }

    Ok(())
}
