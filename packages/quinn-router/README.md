# `packages/router`: Actix-Web Canonical HTTP/3 Routing Engine

> Authoritative architectural breakdown and technical specification of the `router` package. This crate adapts the battle-tested design patterns of **`actix-web`** and **`actix-router`** to multiplexed **HTTP/3 (RFC 9114)** bidirectional streams.

---

## 1. Governing Specifications & Theoretical Foundations

The architecture of this crate is grounded in four authoritative specifications and engineering standards:

### A. Segment-Based URI Matching (RFC 3986 §3.3)
From **RFC 3986 (Uniform Resource Identifier: Generic Syntax)**, Section 3.3:
> *"The path component contains data, usually organized in hierarchical form, that, along with data in the non-hierarchical query component, serves to identify a resource... A path consists of a sequence of path segments separated by a slash ('/')."*

Unlike naive web frameworks that perform raw substring matching or linear regex scans across arbitrary string slices, `packages/router` adheres strictly to RFC 3986 segment boundaries via **`actix_router::ResourceDef`**:
- A pattern `"/api/v0"` defines two discrete segments: `["api", "v0"]`.
- A path `"/api/v0/health"` defines three discrete segments: `["api", "v0", "health"]`.
- Slashes are syntactic delimiters, not string contents.

### B. Algorithmic Complexity: Trie Recognition vs. Naive Regex
Most DIY routing implementations use a linear vector of regular expressions, leading to degraded performance:
$$\text{Linear Regex Dispatch: } \mathcal{O}(N \times K)$$
*(where $N$ is total registered routes and $K$ is average path length).*

`packages/router` delegates dispatching to **`actix_router::Router<HandlerEntry>`**, which compiles route definitions into an internal state machine (Trie):
$$\text{Actix Trie Dispatch: } \mathcal{O}(K)$$
*(where lookup time depends strictly on the segment count $K$ of the incoming URI, remaining completely invariant to whether the server hosts 5 routes or 5,000 routes).*

### C. The Composite & Factory Patterns (Gang of Four)
To achieve complete decoupling without "ripple effects", the router employs two classical design patterns:
1. **Abstract Factory Pattern (`HttpServiceFactory`)**: Handlers and Scopes do not expose concrete internal structures. Instead, they expose a single contract: `fn register(self, config: &mut ServiceConfig)`.
2. **Composite Pattern**: The router tree forms a directed acyclic graph where both leaf nodes (`Route`) and intermediate composite nodes (`Scope`) implement the identical `HttpServiceFactory` interface. The root `App` treats both identically.

---

## 2. Top-Down Module Architecture

The package is strictly divided into six single-responsibility modules:

```
packages/router/src/
├── lib.rs        <── [Level 1] Public API & web::scope facade
├── app.rs        <── [Level 2] Root Application Builder (App)
├── scope.rs      <── [Level 3] Intermediate Scope Grouper (web::scope)
├── config.rs     <── [Level 4] Configuration Context & Factory Trait
├── route.rs      <── [Level 5] Leaf Endpoint Constructors (get, post)
├── service.rs    <── [Level 6] Runtime Trie Dispatcher (Router::dispatch)
└── types.rs      <── [Level 7] QUIC & Async Stream Aliases
```

### Module Descriptions:

#### `types.rs`: Async Transport Types
Defines the memory and future layout for HTTP/3 stream handling:
- **`H3Stream`**: `h3::server::RequestStream<h3_quinn::BidiStream<Bytes>, Bytes>`. A bidirectional stream handle backed by Quinn over UDP.
- **`HandlerFn`**: `Arc<dyn Fn(Request<()>, H3Stream) -> BoxFuture<'static, ...> + Send + Sync>`. An atomically reference-counted closure capable of being dispatched across any Tokio worker thread.

#### `config.rs`: The Core Factory Contract
Defines the mechanism that enables zero-ripple-effect extensibility:
```rust
pub trait HttpServiceFactory {
    fn register(self, config: &mut ServiceConfig);
}
```
`ServiceConfig` acts as the mutable registration builder. It carries:
- `prefix`: The inherited URL segment prefix (e.g. `"/api/v0"`).
- `routes`: The accumulator vector of `(ResourceDef, HandlerEntry)`.

#### `scope.rs`: Path Isolation (Composite Node)
From official `actix-web` source (`actix-web/src/scope.rs`):
> *"A collection of Routes, Resources, or other services that share a common path prefix... Avoid using trailing slashes in the scope prefix."*

When a `Scope` registers into a parent `ServiceConfig`, it concatenates its prefix to all child resource patterns:
```rust
impl HttpServiceFactory for Scope {
    fn register(self, parent: &mut ServiceConfig) {
        let parent_prefix = parent.prefix.trim_end_matches('/');
        for (rdef, entry) in self.config.routes {
            let combined = format!("{}{}", parent_prefix, rdef.pattern().unwrap_or(""));
            parent.routes.push((ResourceDef::new(combined), entry));
        }
    }
}
```

#### `route.rs`: Leaf Endpoints
Constructs the terminal nodes of the tree:
- `get(path, handler)`: Pairs an `http::Method::GET` guard with the function pointer.
- `post(path, handler)`: Pairs an `http::Method::POST` guard with the function pointer.
When registered, it joins `config.prefix` with its own `path` to yield the final `actix_router::ResourceDef`.

#### `app.rs`: Macroscopic Builder
The root entry point matching `actix_web::App`:
- `.configure(f)`: Passes `&mut ServiceConfig` to modular functions.
- `.finish()`: Freezes the accumulated routes, compiles them into `actix_router::RouterBuilder`, and outputs the read-only runtime `Router`.

#### `service.rs`: Runtime Trie Dispatcher
The performance-critical path executed on every incoming HTTP/3 stream:
1. Wraps the incoming URI path in `actix_router::Path`.
2. Queries `self.actix_router.recognize(&mut path)`.
3. Verifies HTTP Method (`GET`, `POST`).
4. Executes the handler or emits a standards-compliant **RFC 9110 §15.5.5 (404 Not Found)** JSON payload.

---

## 3. Mathematical Proof of Zero Ripple Effect

Let $\mathcal{S}_1$ be a scope defined with prefix $P_1 = \text{"/api/v0"}$.
Let $\mathcal{R}_1$ be a leaf route defined with local path $p_1 = \text{"/health"}$.

### 1. Invariance of the Leaf:
$$\mathcal{R}_1 = f(p_1, \text{handler})$$
$\mathcal{R}_1$ contains **zero knowledge** of $P_1$. It can be mounted under `/api/v0`, `/api/v1`, or at the root `/` without altering a single byte of `routes/health.rs`.

### 2. Composition at the Scope:
$$\mathcal{S}_1(P_1) = \bigcup_{i} (P_1 \oplus p_i)$$
The scope acts as an isolated namespace transformation. Adding or deleting a route inside $\mathcal{S}_1$ does not recompile or invalidate sibling scopes $\mathcal{S}_2$.

### 3. Separation from Server Boot (`main.rs`):
`main.rs` only receives a function pointer satisfying `FnOnce(&mut ServiceConfig)`. `main.rs` is completely agnostic to how many routes, sub-scopes, or handlers exist within the application.
