use std::fs::File;
use std::io::BufReader;
use std::sync::Arc;
use std::time::Duration;

use quinn::{ServerConfig, TransportConfig};
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
pub use utils::{AppEnv, ServerEnv};

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("TLS error: {0}")]
    Tls(#[from] rustls::Error),

    #[error("Certificate generation error: {0}")]
    CertGen(#[from] rcgen::Error),

    #[error("QUIC crypto error: {0}")]
    QuicCrypto(String),

    #[error("Invalid configuration: {0}")]
    Invalid(String),
}

pub fn create_server_config(env: &ServerEnv) -> Result<ServerConfig, ConfigError> {
    // 1. Resolve TLS certificates (Disk files in Prod, in-memory self-signed in Dev)
    let (certs, key) = match env.app_env {
        AppEnv::Production => {
            let cert_path = env.tls_cert_path.as_deref().expect("Validated by ServerEnv");
            let key_path = env.tls_key_path.as_deref().expect("Validated by ServerEnv");
            load_prod_certs(cert_path, key_path)?
        }
        AppEnv::Development => generate_dev_certs()?,
    };

    // 2. Build Rustls TLS 1.3 server config
    let mut tls_config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)?;

    // HTTP/3 RFC 9114 strictly mandates ALPN identifier "h3"
    tls_config.alpn_protocols = vec![b"h3".to_vec()];

    // 3. QUIC Transport Tuning from environment:
    let mut transport = TransportConfig::default();
    
    // Drop dead connections if no packet arrives within idle_timeout_secs (reclaims RAM)
    transport.max_idle_timeout(Some(
        Duration::from_secs(env.idle_timeout_secs)
            .try_into()
            .map_err(|e: quinn::VarIntBoundsExceeded| ConfigError::Invalid(e.to_string()))?
    ));
    
    // Keep NAT firewall punch-holes open every keep_alive_interval_secs (prevents router drops)
    transport.keep_alive_interval(Some(Duration::from_secs(env.keep_alive_interval_secs)));

    // 4. Assemble final Quinn ServerConfig
    let quic_crypto = quinn::crypto::rustls::QuicServerConfig::try_from(tls_config)
        .map_err(|e| ConfigError::QuicCrypto(e.to_string()))?;

    let mut server_config = ServerConfig::with_crypto(Arc::new(quic_crypto));
    server_config.transport_config(Arc::new(transport));

    Ok(server_config)
}

/// Generates an ephemeral in-memory self-signed certificate for local development
fn generate_dev_certs() -> Result<(Vec<CertificateDer<'static>>, PrivateKeyDer<'static>), ConfigError> {
    let cert = rcgen::generate_simple_self_signed(vec!["localhost".into()])?;
    let cert_der = CertificateDer::from(cert.cert.der().to_vec());
    let key_der = PrivateKeyDer::Pkcs8(cert.key_pair.serialize_der().into());
    Ok((vec![cert_der], key_der))
}

/// Reads real PEM-encoded certificate chain and PKCS8/RSA private key from disk
fn load_prod_certs(
    cert_path: &str,
    key_path: &str,
) -> Result<(Vec<CertificateDer<'static>>, PrivateKeyDer<'static>), ConfigError> {
    // 1. Read Certificate Chain (e.g. fullchain.pem)
    let cert_file = File::open(cert_path)?;
    let mut cert_reader = BufReader::new(cert_file);
    let certs: Vec<CertificateDer<'static>> = rustls_pemfile::certs(&mut cert_reader)
        .collect::<Result<Vec<_>, _>>()?;

    if certs.is_empty() {
        return Err(ConfigError::Invalid(format!("No certificates found in '{}'", cert_path)));
    }

    // 2. Read Private Key (supports PKCS8, PKCS1 RSA, and SEC1 ECC)
    let key_file = File::open(key_path)?;
    let mut key_reader = BufReader::new(key_file);
    let key = rustls_pemfile::private_key(&mut key_reader)?
        .ok_or_else(|| ConfigError::Invalid(format!("No private key found in '{}'", key_path)))?;

    Ok((certs, key))
}
