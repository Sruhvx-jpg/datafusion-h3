use std::net::SocketAddr;
use std::sync::Arc;
use bytes::Buf;
use http::Request;
use quinn::crypto::rustls::QuicClientConfig;
use quinn::Endpoint;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Build TLS config accepting self-signed dev certs with ALPN "h3"
    let mut crypto = rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(SkipServerVerification))
        .with_no_client_auth();
    crypto.alpn_protocols = vec![b"h3".to_vec()];

    let client_config = quinn::ClientConfig::new(Arc::new(QuicClientConfig::try_from(crypto)?));

    // 2. Bind local UDP client socket
    let mut endpoint = Endpoint::client("0.0.0.0:0".parse()?)?;
    endpoint.set_default_client_config(client_config);

    // 3. Connect to specified port or default to Gateway 4430
    let port = std::env::args().nth(1).unwrap_or_else(|| "4430".to_string());
    let target_addr: SocketAddr = format!("127.0.0.1:{}", port).parse()?;
    println!("Connecting to HTTP/3 at https://{}...", target_addr);

    let conn = endpoint.connect(target_addr, "localhost")?.await?;
    let quinn_conn = h3_quinn::Connection::new(conn);
    let (mut driver, mut send_request) = h3::client::new(quinn_conn).await?;

    tokio::spawn(async move {
        let _ = driver.wait_idle().await;
    });

    // 4. Send GET /api/v0/health request
    let req = Request::builder()
        .method("GET")
        .uri(format!("https://localhost:{}/api/v0/health", port))
        .body(())?;

    println!("Sending GET /api/v0/health...");
    let mut stream = send_request.send_request(req).await?;
    stream.finish().await?;

    // 5. Read HTTP/3 Response
    let resp = stream.recv_response().await?;
    println!("Status: {}", resp.status());
    for (k, v) in resp.headers() {
        println!("{}: {}", k, v.to_str().unwrap_or(""));
    }

    let mut body = Vec::new();
    while let Some(mut chunk) = stream.recv_data().await? {
        while chunk.has_remaining() {
            let b = chunk.chunk();
            body.extend_from_slice(b);
            chunk.advance(b.len());
        }
    }

    println!("Body: {}", String::from_utf8_lossy(&body));
    Ok(())
}

#[derive(Debug)]
struct SkipServerVerification;

impl rustls::client::danger::ServerCertVerifier for SkipServerVerification {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        vec![
            rustls::SignatureScheme::RSA_PKCS1_SHA256,
            rustls::SignatureScheme::ECDSA_NISTP256_SHA256,
            rustls::SignatureScheme::ED25519,
        ]
    }
}
