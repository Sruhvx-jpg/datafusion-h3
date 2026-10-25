use std::env;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppEnv {
    Development,
    Production,
}

#[derive(Debug, Clone)]
pub struct ServerEnv {
    pub host: String,
    pub port: u16,
    pub app_env: AppEnv,
    pub idle_timeout_secs: u64,
    pub keep_alive_interval_secs: u64,
    pub tls_cert_path: Option<String>,
    pub tls_key_path: Option<String>,
}

impl ServerEnv {
    pub fn parse() -> Self {
        // 1. Load .env file
        let _ = dotenvy::dotenv();

        // 2. APP_ENV: Hard panic if missing or invalid
        let app_env_raw = env::var("APP_ENV")
            .unwrap_or_else(|_| panic!("❌ [FATAL CONFIG] Missing required env 'APP_ENV' (must be 'development' or 'production')"));

        let app_env = match app_env_raw.to_lowercase().as_str() {
            "development" | "dev" => AppEnv::Development,
            "production" | "prod" => AppEnv::Production,
            other => panic!("❌ [FATAL CONFIG] Invalid APP_ENV '{}': must be 'development' or 'production'", other),
        };

        // 3. HOST: Hard panic if missing
        let host = env::var("HOST")
            .unwrap_or_else(|_| panic!("❌ [FATAL CONFIG] Missing required env 'HOST'"));

        // 4. PORT: Hard panic if missing or invalid u16
        let port_raw = env::var("PORT")
            .unwrap_or_else(|_| panic!("❌ [FATAL CONFIG] Missing required env 'PORT'"));
        let port = port_raw
            .parse::<u16>()
            .unwrap_or_else(|_| panic!("❌ [FATAL CONFIG] Invalid PORT '{}': must be a valid u16 (1-65535)", port_raw));

        // 5. QUIC Timeouts (defaults: 30s idle, 10s keepalive)
        let idle_timeout_secs = env::var("IDLE_TIMEOUT_SECS")
            .unwrap_or_else(|_| "30".to_string())
            .parse::<u64>()
            .unwrap_or_else(|_| panic!("❌ [FATAL CONFIG] Invalid IDLE_TIMEOUT_SECS: must be a valid u64"));

        let keep_alive_interval_secs = env::var("KEEP_ALIVE_SECS")
            .unwrap_or_else(|_| "10".to_string())
            .parse::<u64>()
            .unwrap_or_else(|_| panic!("❌ [FATAL CONFIG] Invalid KEEP_ALIVE_SECS: must be a valid u64"));

        // 6. TLS Paths
        let tls_cert_path = env::var("TLS_CERT_PATH").ok().filter(|s| !s.trim().is_empty());
        let tls_key_path = env::var("TLS_KEY_PATH").ok().filter(|s| !s.trim().is_empty());

        // 7. Hard panic: In Production, real TLS cert and key are MANDATORY
        if app_env == AppEnv::Production && (tls_cert_path.is_none() || tls_key_path.is_none()) {
            panic!("❌ [FATAL CONFIG] Production mode requires both 'TLS_CERT_PATH' and 'TLS_KEY_PATH' to be set");
        }

        Self {
            host,
            port,
            app_env,
            idle_timeout_secs,
            keep_alive_interval_secs,
            tls_cert_path,
            tls_key_path,
        }
    }
}
