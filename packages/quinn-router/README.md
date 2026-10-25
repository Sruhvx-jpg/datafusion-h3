# `quinn-router` (Actix-Web HTTP/3 Routing Engine)

> An isolated, modular HTTP/3 routing engine implementing **Actix-Web's canonical Factory pattern**, **`actix-router` Trie matching**, **pure Request $\to$ Response ergonomics**, and **automatic DTO validation**, guaranteeing **zero ripple effects** across nested scopes.

---

## 1. Top-Down Architecture: Request $\to$ Response Pipeline

```
[ Client: GET /api/v0/health or POST /api/v0/login ]
                         │
                         ▼ UDP :4433 (RFC 9114 HTTP/3)
[ Level 1: Central Router (`service.rs`) ]
  • Dispatches incoming stream using `actix_router::Router` Trie ($O(K)$ lookup)
  • Wraps raw QUIC stream into high-level `HttpRequest`
                         │
                         ▼
[ Level 2: The Leaf Handler (Pure Business Logic) ]
  • Receives: `&mut HttpRequest`
  • Calls: `req.json::<DTO>().await?` (Auto-deserialization + `validator` schema check)
  • Executes: Internal service function
  • Returns: `HttpResponse::Ok().json(data)` or `Result<HttpResponse, ApiError>`
                         │
                         ▼
[ Level 3: Automatic Wire Transmission ]
  • Router receives `HttpResponse`
  • Automatically encodes QPACK headers, writes DATA frames, and sends QUIC FIN
```

---

## 2. The Abstraction Hierarchy: Types & Handlers

### A. The Request: `HttpRequest` (`internal/request.rs`)
Eliminates manual stream plumbing. Gives Express-style DTO extraction with compile-time validation:

```rust
// In your handler:
let LoginRequest { email, password } = req.json::<LoginRequest>().await?;
```

- **`read_body().await`**: Asynchronously streams raw body bytes from the underlying HTTP/3 bidirectional stream into a sliding memory buffer.
- **`json::<T>().await`**:
  1. Deserializes bytes via `serde_json::from_slice`.
  2. Runs `validator::Validate::validate(&dto)` automatically.
  3. If invalid, early-exits with `400 Bad Request` and descriptive validation errors.
- **Stream Preservation**: Preserves the bidirectional stream handle so the router can write the response back without stream loss.

---

### B. The Response: `HttpResponse` (`internal/response.rs`)
Replicates Actix-Web's iconic response builder syntax:

```rust
// 1. Success with JSON
HttpResponse::Ok().json(json!({ "status": "healthy" }))

// 2. Empty Body
HttpResponse::NoContent().finish()

// 3. Client Errors
HttpResponse::BadRequest().json(json!({ "error": "Invalid syntax" }))
HttpResponse::NotFound().finish()
```

#### Supported Static Constructors:
`Ok()`, `Created()`, `Accepted()`, `NoContent()`, `BadRequest()`, `Unauthorized()`, `Forbidden()`, `NotFound()`, `InternalServerError()`.

---

### C. Standardized Error Handling: `ApiError` (`internal/error.rs`)
A production-ready error envelope unifying all failure modes into a single contract:

```rust
#[derive(Debug, Clone, Serialize)]
pub struct ApiError {
    #[serde(skip)]
    pub status: StatusCode,
    pub message: String,
}
```

#### Constructors:
- `ApiError::bad_request(msg)` $\to$ `400 Bad Request`
- `ApiError::unauthorized(msg)` $\to$ `401 Unauthorized`
- `ApiError::forbidden(msg)` $\to$ `403 Forbidden`
- `ApiError::not_found(msg)` $\to$ `404 Not Found`
- `ApiError::internal(msg)` $\to$ `500 Internal Server Error`

#### The `?` Operator Duality (`IntoHttpResponse`):
Because `ApiError` implements `IntoHttpResponse`, any fallible internal method can use the standard Rust `?` operator:

```rust
#[post("/login")]
pub async fn login(req: &mut HttpRequest) -> Result<HttpResponse, ApiError> {
    let payload = req.json::<LoginRequest>().await?; // Auto-emits 400 if invalid
    let user = authenticate(&payload.email, &payload.password).await?; // Auto-emits 401 if wrong
    Ok(HttpResponse::Ok().json(user)) // Auto-emits 200 on success
}
```

---

## 3. Internal Module Directory Layout

```
packages/quinn-router/src/
├── lib.rs              <── Public Facade: re-exports App, Scope, macros, HttpResponse, ApiError
├── router.rs           <── Central /api/v0 definition: mounts child routes
├── handlers/           <── Feature Leaf Handlers
│   └── health.rs       <── health_check handler using #[get("/health")]
│
└── internal/           <── 🎯 Engine Primitives:
    ├── mod.rs          <── Internal module declarations
    ├── app.rs          <── App builder & .finish()
    ├── config.rs       <── ServiceConfig & HttpServiceFactory trait
    ├── error.rs        <── ApiError standardized error struct & constructors
    ├── request.rs      <── HttpRequest wrapper with .json::<DTO>() & validator
    ├── response.rs     <── HttpResponse, HttpResponseBuilder & IntoHttpResponse
    ├── route.rs        <── Route struct & method constructors (get, post, put...)
    ├── scope.rs        <── Scope & web::scope prefix isolation
    ├── service.rs      <── Router runtime Trie dispatcher (actix_router)
    └── types.rs        <── H3Stream, BoxFuture, HandlerFn aliases
```

---

## 4. Verification

Test over HTTP/3 over QUIC directly via Snap curl:
```bash
/snap/bin/curl --http3-only --insecure https://127.0.0.1:4433/api/v0/health
```

Output:
```json
{"service":"service4agent","status":"healthy"}
```
