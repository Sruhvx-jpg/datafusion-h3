# Service4Agent (DataFusion-H3)

> High-throughput, low-latency analytical microservice for AI agents, built natively on **HTTP/3 (QUIC)**, **Apache DataFusion**, and **Actix-Web's modular routing architecture**.

---

## 1. Top-Down Architecture

The workspace is structured strictly hierarchically from the macroscopic network gateway down to individual leaf route handlers:

```
                          [ Client / AI Agent / Benchmark ]
                                         │
                                         ▼ UDP :4430 (RFC 9114)
                     ┌───────────────────────────────────────┐
                     │          apps/gateway                 │
                     │  • Public Front-Door Gateway          │
                     │  • Lock-Free 50/50 Round-Robin (lb)   │
                     └───────────────────┬───────────────────┘
                                         │
                      ┌──────────────────┴──────────────────┐
                      ▼ UDP :4433                           ▼ UDP :4434
       ┌─────────────────────────────┐       ┌─────────────────────────────┐
       │ apps/quinn-server           │       │ apps/s2n-server (Showdown)  │
       │ • Quinn + Rustls Backend    │       │ • AWS s2n-quic Backend      │
       │ • Actix App + web::scope    │       │ • Formally Verified Crypto  │
       └──────────────┬──────────────┘       └──────────────┬──────────────┘
                      │                                     │
                      └──────────────────┬──────────────────┘
                                         ▼
                     ┌───────────────────────────────────────┐
                     │          packages/router              │
                     │  • actix_router::Router (Trie Engine) │
                     │  • HttpServiceFactory Trait           │
                     │  • web::scope("/api/v0")              │
                     └───────────────────┬───────────────────┘
                                         │
                                         ▼
                     ┌───────────────────────────────────────┐
                     │          routes/health.rs             │
                     │  • Leaf Handler: 200 OK JSON          │
                     └───────────────────────────────────────┘
```

---

## 2. Authoritative Specifications & Standards

This codebase strictly implements and cross-references governing IETF RFC specifications and Actix-Web internal standards:

| Authority / Spec | Section / Clause | How Service4Agent Implements It |
| :--- | :--- | :--- |
| **RFC 9114 (HTTP/3)** | **Section 3.1 (Connection Establishment)**<br>*"The HTTP/3 protocol uses the ALPN token 'h3'..."* | Enforced in `packages/server-conf/src/lib.rs` via `tls_config.alpn_protocols = vec![b"h3".to_vec()];`. Connections lacking this token are rejected at the TLS handshake. |
| **RFC 9114 (HTTP/3)** | **Section 6 (Stream Mapping)**<br>*"A client sends an HTTP request on a request stream, which is a bidirectional QUIC stream..."* | Bidirectional streams are accepted in the main event loop via `h3_conn.accept().await` and resolved via `resolver.resolve_request().await`. |
| **RFC 9000 (QUIC)** | **Section 10.1 (Idle Timeout)**<br>*"If a max_idle_timeout is specified by either endpoint, the connection is silently closed after that duration..."* | Configured in `TransportConfig::max_idle_timeout` using `IDLE_TIMEOUT_SECS` from `.env` to reclaim dead socket memory. |
| **RFC 9000 (QUIC)** | **Section 5.1 (Connection ID Routing)** | Informs the architecture of `apps/gateway`: naive packet-by-packet UDP round-robin is forbidden in QUIC because backends do not share TLS session state. The gateway terminates the stream or routes deterministically. |
| **Actix Web Architecture** | **Trie Path Matching (`actix-router`)** | `packages/router` uses `actix_router::Router<HandlerEntry>` and `actix_router::ResourceDef` for zero-allocation static and parameterized path matching. |
| **Actix Web Architecture** | **Factory Pattern (`HttpServiceFactory`)** | Replicates Actix-Web's `HttpServiceFactory` and `ServiceConfig` pattern to guarantee **zero ripple effects** when mounting nested scopes. |

---

## 3. Workspace Layout & Package Topology

```
datafusion-h3/
├── Cargo.toml                  <-- Root Cargo Workspace manifest
├── .env                        <-- Fail-fast environment configuration
├── .env.example                <-- Production environment template
│
├── apps/
│   ├── gateway/                <-- The Upfront Load Balancer Server (UDP :4430)
│   │   ├── Cargo.toml
│   │   └── src/main.rs         <-- Boots gateway and dispatches to backends
│   │
│   └── quinn-server/           <-- Backend A: Quinn HTTP/3 Engine (UDP :4433)
│       ├── Cargo.toml
│       └── src/
│           ├── main.rs         <-- Boots server & mounts App::new().configure(...)
│           └── routes/
│               ├── mod.rs      <-- Base Configurator: mounts /api/v0 scope
│               └── health.rs   <-- Leaf Handler: GET /api/v0/health
│
└── packages/
    ├── router/                 <-- Actix-Web Modular Routing Engine
    │   ├── Cargo.toml          <-- Depends on actix-router = "0.5"
    │   └── src/
    │       ├── lib.rs          <-- Public API re-exports
    │       ├── app.rs          <-- App builder & .finish() -> Router
    │       ├── scope.rs        <-- Scope prefix isolation (web::scope)
    │       ├── config.rs       <-- ServiceConfig & HttpServiceFactory trait
    │       ├── route.rs        <-- Route struct & get(), post() constructors
    │       ├── service.rs      <-- Router runtime dispatcher (actix_router Trie)
    │       └── types.rs        <-- H3Stream, BoxFuture, HandlerFn
    │
    ├── lb/                     <-- Lock-Free 50/50 Round-Robin Load Balancer
    │   ├── Cargo.toml
    │   └── src/lib.rs          <-- AtomicUsize round-robin & x-served-by headers
    │
    └── server-conf/            <-- Production TLS 1.3 & QUIC Configuration
        ├── Cargo.toml          <-- Quinn 0.11, Rustls 0.23, RCGen, Rustls-Pemfile
        ├── src/lib.rs          <-- Dual-mode TLS (disk PEM in Prod, RAM in Dev)
        └── utils/              <-- Internal crate: Fail-fast .env parser
            ├── Cargo.toml
            └── src/
                ├── lib.rs
                └── env_parser.rs <-- Panics immediately on missing/invalid config
```

---

## 4. The "Zero Ripple Effect" Mounting Tree

Adding or modifying endpoints requires zero changes to `main.rs` or sibling modules. Everything flows strictly top-down:

```rust
// 1. In apps/quinn-server/src/main.rs: Mount base configurator once
let router = App::new()
    .configure(api_v0_config)
    .finish();

// 2. In apps/quinn-server/src/routes/mod.rs: Mount feature scopes
pub fn api_v0_config(cfg: &mut ServiceConfig) {
    cfg.service(
        web::scope("/api/v0")
            .service(get("/health", health_handler))
            // .configure(analytics_routes) <── Add future domains here
    );
}

// 3. In apps/quinn-server/src/routes/health.rs: Pure business logic
pub async fn health_handler(_req: Request<()>, mut stream: H3Stream) -> Result<(), ...> {
    // Write 200 OK + JSON
}
```

---

## 5. Running & Verifying

### Run the Backend (Quinn HTTP/3 on `:4433`):
```bash
cargo run -p quinn-server
```
Output:
```
INFO server: 🚀 [Service4Agent] Running on UDP https://127.0.0.1:4433 (HTTP/3)
```

### Run the Upfront Gateway (Load Balancer on `:4430`):
```bash
cargo run -p gateway
```
Output:
```
INFO gateway: 🌐 [Gateway LB] Listening on UDP https://127.0.0.1:4430
```

### Test over HTTP/3 with `curl`:
```bash
curl --http3 -k https://127.0.0.1:4430/api/v0/health
```
Response:
```json
{"status":"healthy","service":"service4agent"}
```
Response Header:
```http
x-served-by: quinn
```
