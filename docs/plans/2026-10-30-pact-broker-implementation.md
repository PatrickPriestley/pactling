# Pactling Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** A single-binary, SQLite-backed Pact broker that real pact-js and `pact-broker` CLI clients use unchanged, through publish, verify, can-i-deploy and webhooks.

**Architecture:** Three layers in one crate: axum handlers that render HAL JSON (`src/http/`), pure service functions over domain types (`src/service/`), and plain SQL in one module (`src/db/`). Fidelity to the Ruby broker is proven by replaying captured fixtures from `tests/fixtures/ruby-broker/<version>/`, and acceptance is proven by running the real clients against the binary in `e2e/`.

**Tech Stack:** Rust 2024 edition, axum 0.8, sqlx 0.9 (SQLite, tokio runtime, migrate), tokio 1, serde/serde_json, clap 4, reqwest 0.13 (webhooks), sha2, uuid, chrono, thiserror, tracing. Clients: `@pact-foundation/pact-cli` 16 (standalone `pact-broker` 1.77), `@pact-foundation/pact` 13.

Design: `docs/plans/2026-10-30-pact-broker-design.md`.

---

## Evidence gathered before this plan

These are verified against the real system (Ruby broker image `pactfoundation/pact-broker`, reported version **2.121.2**, started from `pact-broker-infra`'s compose file with `PACT_BROKER_SEED_EXAMPLE_DATA=false`), not from docs or memory:

- The index at `/` is the full document in `tests/fixtures/ruby-broker/2.121.2/GET_index.json`. Clients follow its link relations by name. Quirks to reproduce exactly: `pb:tagged-pact-versions` carries `"templated": false` despite a template; `pb:metrics` has no `templated` key; `beta:provider-pacts-for-verification` carries `"name": "beta"`; `curies` is an array of two entries.
- Success and HAL error responses use `content-type: application/hal+json;charset=utf-8`. Route-level 404s (`/pacticipants/Nope/versions/1`, `/environments/nope`, `/webhooks/nope`, unknown latest pact) use `application/json;charset=utf-8` with body `{"error":"The requested document was not found on this server."}`. A resource-level 404 for an unknown pacticipant is HAL with `{"error":"No pacticipant with name 'Nope' found"}`.
- `GET /matrix` with no selectors: 400, `{"errors":["Please provide 1 or more version selectors."]}`. `GET /can-i-deploy` with no params: 400 with field-keyed errors (see fixture). The CLI's `can-i-deploy` calls `/matrix`, not `/can-i-deploy`.
- `HEAD /` returns 405 with `allow: GET, OPTIONS`. Every response carries `x-pact-broker-version` and `x-content-type-options: nosniff`.
- hrefs are absolute and built from the request's `Host` header (`http://localhost:9292/...`).
- The broker seeds example data on first boot unless `PACT_BROKER_SEED_EXAMPLE_DATA=false`. Pactling does not seed.
- CLI against an empty broker: `describe-pacticipant --name Nope` exits 1 and prints the 404 body; `list-latest-pact-versions` prints `No data.` and exits 0; `list-environments` prints an empty line and exits 0.
- Toolchain on this machine: cargo 1.99, Docker 28, node 22, `pact-broker` CLI 1.77 via `@pact-foundation/pact-cli` 16.1.2.

**Still assumed, to be probed in the task that first needs it:** the 401 body and headers under basic auth; the exact request bodies pact-js 13 and the CLI send to `pb:publish-contracts`, `pb:publish-pact`, `for-verification` and verification-results; matrix row shapes; webhook request shape. Each later phase starts with a capture task.

A Ruby broker for probing: `docker compose -p pactling-probe -f scripts/ruby-broker/docker-compose.yml up -d` (Task 1.9 adds that file). It listens on `http://localhost:9292`.

---

## Conventions for every task

- TDD: write the test, run it, see it fail on the assertion (not on a compile error of the test itself; a missing function is acceptable only for the very first test of a module), implement, see it pass, commit.
- Commits are Conventional Commits with no AI attribution lines.
- Mutation-check each guard you add: break it, see a test fail, restore it.
- Fixture tests compare JSON after replacing `http://localhost:9292` in the fixture with the test server's base URL, and ignore keys named `createdAt`, `updatedAt`, `uuid`.
- Run `cargo fmt` and `cargo clippy --all-targets -- -D warnings` before each commit.

---

# Phase 1: Skeleton

End state: `pactling` serves the HAL index and all empty-state documents byte-for-byte equivalent to the fixtures, enforces basic auth, and the real CLI's `describe-pacticipant`, `list-latest-pact-versions` and `list-environments` behave exactly as against the Ruby broker.

### Task 1.1: Crate, dependencies, test harness

**Files:**
- Create: `Cargo.toml`, `src/main.rs`, `src/lib.rs`, `tests/common/mod.rs`, `tests/index.rs`, `.gitignore`, `rust-toolchain.toml`

**Step 1: Create the crate**

```bash
cd /Users/patrickpriestley/Code/pactling
cargo init --name pactling
printf 'target/\n*.db\n*.db-*\n.testsprite/\n' > .gitignore
printf '[toolchain]\nchannel = "stable"\n' > rust-toolchain.toml
```

**Step 2: Write `Cargo.toml`**

```toml
[package]
name = "pactling"
version = "0.1.0"
edition = "2024"
description = "A Pact broker in one binary"
license = "MIT"

[dependencies]
axum = "0.8"
tokio = { version = "1", features = ["full"] }
sqlx = { version = "0.9", features = ["sqlite", "runtime-tokio", "migrate", "chrono", "uuid"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
clap = { version = "4", features = ["derive", "env"] }
thiserror = "2"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
chrono = { version = "0.4", features = ["serde"] }
uuid = { version = "1", features = ["v4", "serde"] }
sha2 = "0.11"
tower-http = { version = "0.7", features = ["trace"] }
base64 = "0.22"

[dev-dependencies]
reqwest = { version = "0.13", features = ["json"] }
```

**Step 3: Write the failing test**

`tests/common/mod.rs`:

```rust
use pactling::{app, config::Config};
use std::net::SocketAddr;

pub struct TestServer {
    pub base_url: String,
    pub client: reqwest::Client,
}

pub async fn start() -> TestServer {
    start_with(Config::in_memory()).await
}

pub async fn start_with(config: Config) -> TestServer {
    let router = app::build(config).await.expect("app builds");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr: SocketAddr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    TestServer {
        base_url: format!("http://{addr}"),
        client: reqwest::Client::new(),
    }
}

/// Load a Ruby-broker fixture and rewrite its hrefs to point at `base_url`.
pub fn fixture(name: &str, base_url: &str) -> serde_json::Value {
    let path = format!(
        "{}/tests/fixtures/ruby-broker/2.121.2/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    let raw = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    serde_json::from_str(&raw.replace("http://localhost:9292", base_url)).unwrap()
}

/// Remove volatile keys before comparing two documents.
pub fn stable(mut v: serde_json::Value) -> serde_json::Value {
    fn walk(v: &mut serde_json::Value) {
        match v {
            serde_json::Value::Object(m) => {
                m.remove("createdAt");
                m.remove("updatedAt");
                m.remove("uuid");
                m.values_mut().for_each(walk);
            }
            serde_json::Value::Array(a) => a.iter_mut().for_each(walk),
            _ => {}
        }
    }
    walk(&mut v);
    v
}
```

`tests/index.rs`:

```rust
mod common;

#[tokio::test]
async fn index_matches_ruby_broker_fixture() {
    let s = common::start().await;
    let res = s.client.get(&s.base_url).send().await.unwrap();
    assert_eq!(res.status(), 200);
    assert_eq!(
        res.headers()["content-type"],
        "application/hal+json;charset=utf-8"
    );
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body, common::fixture("GET_index.json", &s.base_url));
}
```

**Step 4: Write the minimal `src/lib.rs`, `src/config.rs`, `src/app.rs`, `src/main.rs`**

`src/lib.rs`:

```rust
pub mod app;
pub mod config;
```

`src/config.rs`:

```rust
use clap::Parser;

#[derive(Parser, Debug, Clone)]
#[command(name = "pactling", version, about = "A Pact broker in one binary")]
pub struct Config {
    /// Port to listen on
    #[arg(long, env = "PACT_BROKER_PORT", default_value_t = 9292)]
    pub port: u16,

    /// SQLite database URL. Use `sqlite::memory:` for an in-memory database.
    #[arg(long, env = "PACTLING_DATABASE_URL", default_value = "sqlite://pactling.db?mode=rwc")]
    pub database_url: String,
}

impl Config {
    pub fn in_memory() -> Self {
        Self { port: 0, database_url: "sqlite::memory:".into() }
    }
}
```

`src/app.rs`:

```rust
use crate::config::Config;
use axum::{Router, routing::get};

pub async fn build(_config: Config) -> anyhow::Result<Router> {
    Ok(Router::new().route("/", get(|| async { "" })))
}
```

Add `anyhow = "1"` to `[dependencies]`.

`src/main.rs`:

```rust
use clap::Parser;
use pactling::{app, config::Config};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    let config = Config::parse();
    let port = config.port;
    let router = app::build(config).await?;
    let listener = tokio::net::TcpListener::bind(("0.0.0.0", port)).await?;
    tracing::info!("pactling listening on {}", listener.local_addr()?);
    axum::serve(listener, router).await?;
    Ok(())
}
```

**Step 5: Run the test, confirm it fails on the assertion**

Run: `cargo test --test index`
Expected: FAIL at `assert_eq!(res.headers()["content-type"], ...)` with `text/plain`. Not a compile error.

**Step 6: Commit**

```bash
git add -A
git commit -m "chore: crate skeleton with test harness and index fixture test"
```

### Task 1.2: HAL index

**Files:**
- Create: `src/http/mod.rs`, `src/http/hal.rs`, `src/http/index.rs`
- Modify: `src/app.rs`, `src/lib.rs`

**Step 1: Write `src/http/hal.rs`**, the one place that builds links and the HAL response type.

```rust
use axum::{
    http::{HeaderValue, header},
    response::{IntoResponse, Response},
};
use serde_json::{Map, Value, json};

pub const HAL_CONTENT_TYPE: &str = "application/hal+json;charset=utf-8";
pub const JSON_CONTENT_TYPE: &str = "application/json;charset=utf-8";

/// A HAL document. Serialises with `content-type: application/hal+json;charset=utf-8`.
pub struct Hal(pub Value);

impl IntoResponse for Hal {
    fn into_response(self) -> Response {
        let mut res = axum::Json(self.0).into_response();
        res.headers_mut()
            .insert(header::CONTENT_TYPE, HeaderValue::from_static(HAL_CONTENT_TYPE));
        res
    }
}

/// Builds absolute hrefs from the request's base URL, as the Ruby broker does.
#[derive(Clone, Debug)]
pub struct Links {
    base: String,
}

impl Links {
    pub fn new(base: impl Into<String>) -> Self {
        Self { base: base.into() }
    }

    pub fn href(&self, path: &str) -> String {
        format!("{}{}", self.base, path)
    }

    pub fn link(&self, path: &str, title: &str, templated: bool) -> Value {
        json!({ "href": self.href(path), "title": title, "templated": templated })
    }

    /// Link without a `templated` key (the Ruby broker omits it for `pb:metrics`).
    pub fn link_untyped(&self, path: &str, title: &str) -> Value {
        json!({ "href": self.href(path), "title": title })
    }

    pub fn object(&self, pairs: Vec<(&str, Value)>) -> Value {
        let mut m = Map::new();
        for (k, v) in pairs {
            m.insert(k.to_string(), v);
        }
        Value::Object(m)
    }
}
```

**Step 2: Write `src/http/base_url.rs`**, an extractor that derives the base URL from `Host` (and `X-Forwarded-Proto` if present), with a config override later.

```rust
use axum::{
    extract::FromRequestParts,
    http::{StatusCode, request::Parts},
};

use super::hal::Links;

impl<S: Send + Sync> FromRequestParts<S> for Links {
    type Rejection = StatusCode;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let host = parts
            .headers
            .get("host")
            .and_then(|h| h.to_str().ok())
            .ok_or(StatusCode::BAD_REQUEST)?;
        let scheme = parts
            .headers
            .get("x-forwarded-proto")
            .and_then(|h| h.to_str().ok())
            .unwrap_or("http");
        Ok(Links::new(format!("{scheme}://{host}")))
    }
}
```

**Step 3: Write `src/http/index.rs`** with every relation from the fixture, in fixture order. Copy titles verbatim from `GET_index.json`. Preserve the three quirks noted above. Example of the start:

```rust
use super::hal::{Hal, Links};
use serde_json::json;

pub async fn index(links: Links) -> Hal {
    let l = &links;
    Hal(json!({
        "_links": {
            "self": l.link("", "Index", false),
            "pb:publish-pact": l.link("/pacts/provider/{provider}/consumer/{consumer}/version/{consumerApplicationVersion}", "Publish a pact", true),
            "pb:publish-contracts": l.link("/contracts/publish", "Publish contracts", false),
            "pb:latest-pact-versions": l.link("/pacts/latest", "Latest pact versions", false),
            "pb:tagged-pact-versions": l.link("/pacts/provider/{provider}/consumer/{consumer}/tag/{tag}", "All versions of a pact for a given consumer, provider and consumer version tag", false),
            // ... every remaining relation from the fixture, in order ...
            "pb:metrics": l.link_untyped("/metrics", "Get Pact Broker metrics"),
            // ...
            "beta:provider-pacts-for-verification": { "name": "beta", "href": l.href("/pacts/provider/{provider}/for-verification"), "title": "DEPRECATED - please use pb:provider-pacts-for-verification", "templated": true },
            "curies": [
                { "name": "pb", "href": l.href("/doc/{rel}?context=index"), "templated": true },
                { "name": "beta", "href": l.href("/doc/{rel}?context=index"), "templated": true }
            ],
            "pb:environments": { "title": "Environments", "href": l.href("/environments"), "templated": false },
            "pb:environment": { "title": "Environment", "href": l.href("/environments/{uuid}"), "templated": true }
        }
    }))
}
```

`serde_json::Value` objects are unordered unless the `preserve_order` feature is on. The fixture comparison is on `Value` equality, so key order does not matter to the test, but enable `serde_json = { version = "1", features = ["preserve_order"] }` anyway so that human diffs against the Ruby broker line up.

**Step 4: Wire it** in `src/app.rs`: `Router::new().route("/", get(http::index::index))`. Add a global middleware layer that sets `x-pact-broker-version: <crate version>` and `x-content-type-options: nosniff` on every response (`tower_http::set_header::SetResponseHeaderLayer`, feature `set-header`).

**Step 5: Run the test**

Run: `cargo test --test index`
Expected: PASS. If it fails, print both documents with `serde_json::to_string_pretty` and fix the relation that differs. Do not edit the fixture.

**Step 6: Add a header test** to `tests/index.rs`:

```rust
#[tokio::test]
async fn every_response_carries_broker_headers() {
    let s = common::start().await;
    let res = s.client.get(&s.base_url).send().await.unwrap();
    assert_eq!(res.headers()["x-content-type-options"], "nosniff");
    assert!(res.headers().contains_key("x-pact-broker-version"));
}

#[tokio::test]
async fn head_on_index_is_405_with_allow() {
    let s = common::start().await;
    let res = s.client.head(&s.base_url).send().await.unwrap();
    assert_eq!(res.status(), 405);
    assert_eq!(res.headers()["allow"], "GET, OPTIONS");
}
```

axum answers HEAD for GET routes by default, so the 405 test fails first. Implement with a `MethodRouter` that handles `get` and `options` only and a fallback for the method-not-allowed case that sets `allow`. Run, pass, commit.

**Step 7: Commit**

```bash
git commit -am "feat(http): HAL index matching Ruby broker 2.121.2"
```

### Task 1.3: Errors and the two 404 shapes

**Files:**
- Create: `src/error.rs`, `tests/errors.rs`
- Modify: `src/lib.rs`, `src/app.rs`

**Step 1: Failing tests** in `tests/errors.rs`:

```rust
mod common;

#[tokio::test]
async fn unknown_route_is_plain_json_404() {
    let s = common::start().await;
    let res = s.client.get(format!("{}/environments/nope", s.base_url)).send().await.unwrap();
    assert_eq!(res.status(), 404);
    assert_eq!(res.headers()["content-type"], "application/json;charset=utf-8");
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body, common::fixture("GET_environments_nope.json", &s.base_url));
}

#[tokio::test]
async fn unknown_pacticipant_is_hal_404_with_message() {
    let s = common::start().await;
    let res = s.client.get(format!("{}/pacticipants/Nope", s.base_url)).send().await.unwrap();
    assert_eq!(res.status(), 404);
    assert_eq!(res.headers()["content-type"], "application/hal+json;charset=utf-8");
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body, common::fixture("GET_pacticipants_Nope.json", &s.base_url));
}
```

**Step 2: Run**: `cargo test --test errors`. Expected: both FAIL (404 from axum's default fallback has an empty body and no content-type).

**Step 3: Implement `src/error.rs`**

```rust
use axum::{
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde_json::json;

use crate::http::hal::{HAL_CONTENT_TYPE, JSON_CONTENT_TYPE};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A route exists but the addressed resource does not. Rendered as HAL, with the message.
    #[error("{0}")]
    NotFound(String),
    /// No route matched. Rendered as plain JSON with the Ruby broker's generic message.
    #[error("The requested document was not found on this server.")]
    NoRoute,
    #[error("{0:?}")]
    Validation(serde_json::Value),
    #[error(transparent)]
    Db(#[from] sqlx::Error),
}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        let (status, content_type, body) = match &self {
            Error::NotFound(msg) => (StatusCode::NOT_FOUND, HAL_CONTENT_TYPE, json!({ "error": msg })),
            Error::NoRoute => (StatusCode::NOT_FOUND, JSON_CONTENT_TYPE, json!({ "error": self.to_string() })),
            Error::Validation(errors) => (StatusCode::BAD_REQUEST, HAL_CONTENT_TYPE, json!({ "errors": errors })),
            Error::Db(e) => {
                tracing::error!(error = %e, "database error");
                (StatusCode::INTERNAL_SERVER_ERROR, JSON_CONTENT_TYPE, json!({ "error": "internal error" }))
            }
        };
        let mut res = (status, axum::Json(body)).into_response();
        res.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
        res
    }
}

pub type Result<T> = std::result::Result<T, Error>;
```

Add `.fallback(|| async { Error::NoRoute })` to the router and a stub `GET /pacticipants/{name}` handler returning `Err(Error::NotFound(format!("No pacticipant with name '{name}' found")))` (the real lookup comes in Task 1.5).

**Step 4: Run, pass, mutation-check** (swap the two content types, see both tests fail, restore). **Commit:** `feat(http): error type with Ruby-compatible 404 shapes`.

### Task 1.4: Database and migrations

**Files:**
- Create: `src/db/mod.rs`, `migrations/0001_pacticipants.sql`, `tests/db.rs`
- Modify: `src/app.rs`, `src/lib.rs`

**Step 1: Failing test** in `tests/db.rs`:

```rust
use pactling::db::Db;

#[tokio::test]
async fn migrations_create_pacticipants_table() {
    let db = Db::connect("sqlite::memory:").await.unwrap();
    db.migrate().await.unwrap();
    let names = db.list_pacticipant_names().await.unwrap();
    assert!(names.is_empty());
}
```

**Step 2: Run**: `cargo test --test db`. Expected: FAIL to compile on missing `pactling::db` (acceptable for a module's first test).

**Step 3: Implement**

`migrations/0001_pacticipants.sql`:

```sql
CREATE TABLE pacticipants (
    id             INTEGER PRIMARY KEY,
    name           TEXT NOT NULL UNIQUE,
    display_name   TEXT,
    repository_url TEXT,
    main_branch    TEXT,
    created_at     TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    updated_at     TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);
```

`src/db/mod.rs`:

```rust
use sqlx::sqlite::{SqlitePool, SqlitePoolOptions};

#[derive(Clone)]
pub struct Db {
    pool: SqlitePool,
}

impl Db {
    pub async fn connect(url: &str) -> sqlx::Result<Self> {
        let pool = SqlitePoolOptions::new()
            // One connection keeps an in-memory database alive and serialises SQLite writes.
            .max_connections(if url.contains(":memory:") { 1 } else { 4 })
            .connect(url)
            .await?;
        sqlx::query("PRAGMA foreign_keys = ON").execute(&pool).await?;
        Ok(Self { pool })
    }

    pub async fn migrate(&self) -> sqlx::Result<()> {
        sqlx::migrate!("./migrations").run(&self.pool).await.map_err(Into::into)
    }

    pub async fn list_pacticipant_names(&self) -> sqlx::Result<Vec<String>> {
        let rows: Vec<(String,)> = sqlx::query_as("SELECT name FROM pacticipants ORDER BY name")
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.into_iter().map(|(n,)| n).collect())
    }
}
```

In `app::build`: connect, migrate, and put `Db` in axum state (`Router::with_state(AppState { db, config })`).

**Step 4: Run, pass, commit**: `feat(db): sqlite pool and first migration`.

### Task 1.5: Empty-state collections

**Files:**
- Create: `src/http/pacticipants.rs`, `src/http/environments.rs`, `src/http/webhooks.rs`, `src/http/integrations.rs`, `src/http/pacts.rs`, `src/http/matrix.rs`, `tests/empty_state.rs`
- Modify: `src/app.rs`

**Step 1: Failing tests**, one per fixture, in `tests/empty_state.rs`. Use a helper:

```rust
mod common;

async fn assert_fixture(path: &str, fixture: &str, status: u16) {
    let s = common::start().await;
    let res = s.client.get(format!("{}{path}", s.base_url)).send().await.unwrap();
    assert_eq!(res.status(), status, "{path}");
    assert_eq!(res.headers()["content-type"], "application/hal+json;charset=utf-8", "{path}");
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(common::stable(body), common::stable(common::fixture(fixture, &s.base_url)), "{path}");
}

#[tokio::test] async fn pacticipants() { assert_fixture("/pacticipants", "GET_pacticipants.json", 200).await }
#[tokio::test] async fn environments() { assert_fixture("/environments", "GET_environments.json", 200).await }
#[tokio::test] async fn webhooks() { assert_fixture("/webhooks", "GET_webhooks.json", 200).await }
#[tokio::test] async fn integrations() { assert_fixture("/integrations", "GET_integrations.json", 200).await }
#[tokio::test] async fn latest_pacts() { assert_fixture("/pacts/latest", "GET_pacts_latest.json", 200).await }
#[tokio::test] async fn matrix_without_selectors() { assert_fixture("/matrix", "GET_matrix.json", 400).await }
#[tokio::test] async fn can_i_deploy_without_params() { assert_fixture("/can-i-deploy", "GET_can-i-deploy.json", 400).await }
```

**Step 2: Run**: all seven FAIL with 404.

**Step 3: Implement each handler** to render exactly the fixture's empty document. `GET /pacticipants` reads from `Db::list_pacticipant_names` (empty for now) so the handler is already the real one. The others return static empty documents built with `Links`. `/matrix` and `/can-i-deploy` return `Error::Validation(...)` with the fixture's exact error bodies when their required params are absent; the real evaluation arrives in Phase 4.

**Step 4: Run, pass, commit**: `feat(http): empty-state documents for pacticipants, environments, webhooks, integrations, latest pacts, matrix`.

### Task 1.6: Basic auth

**Files:**
- Create: `src/http/auth.rs`, `tests/auth.rs`, `tests/fixtures/ruby-broker/2.121.2/auth/`
- Modify: `src/config.rs`, `src/app.rs`

**Step 1: Probe the Ruby broker.** Restart it with credentials and capture what clients will see:

```bash
cd scripts/ruby-broker
PACT_BROKER_BASIC_AUTH_USERNAME=rw PACT_BROKER_BASIC_AUTH_PASSWORD=rwpass \
PACT_BROKER_BASIC_AUTH_READ_ONLY_USERNAME=ro PACT_BROKER_BASIC_AUTH_READ_ONLY_PASSWORD=ropass \
PACT_BROKER_ALLOW_PUBLIC_READ=false \
docker compose -p pactling-probe up -d --force-recreate pact-broker
D=../../tests/fixtures/ruby-broker/2.121.2/auth; mkdir -p $D
curl -s -D $D/no-creds.headers -o $D/no-creds.body http://localhost:9292/
curl -s -u ro:ropass -D $D/ro-get.headers -o /dev/null http://localhost:9292/
curl -s -u ro:ropass -X PUT -H 'content-type: application/json' -d '{}' -D $D/ro-put.headers -o $D/ro-put.body http://localhost:9292/pacticipants/X
curl -s -u rw:wrong -D $D/bad-creds.headers -o $D/bad-creds.body http://localhost:9292/
```

Read the captured files and write the tests from them, not from this plan. The plan's expectation, **assumed until the probe runs**: no credentials gives 401 with `www-authenticate: Basic realm="Pact Broker"`; read-only user gets 200 on GET and 403 on PUT; wrong password gives 401.

**Step 2: Failing tests** in `tests/auth.rs` using `common::start_with(Config { basic_auth: Some(...), ..Config::in_memory() })`, asserting the status and headers captured in step 1.

**Step 3: Implement** `Config` fields `basic_auth_username`, `basic_auth_password`, `basic_auth_read_only_username`, `basic_auth_read_only_password`, `allow_public_read` (env names `PACT_BROKER_BASIC_AUTH_USERNAME` etc. and `PACT_BROKER_ALLOW_PUBLIC_READ`), and `bearer_token` (`PACTLING_BEARER_TOKEN`, accepted as `Authorization: Bearer` with read/write rights). Auth is an axum middleware in `src/http/auth.rs` that classifies the request as read (GET, HEAD, OPTIONS) or write and checks the credentials against the configured set. When no credentials are configured, everything is allowed.

**Step 4: Run, pass.** Mutation-check: invert the read/write classification and the password comparison in turn; a test must fail for each. **Commit**: `feat(auth): basic auth with read-only user and bearer token`.

### Task 1.7: Pacticipant lookup and `describe-pacticipant`

**Files:**
- Modify: `src/db/mod.rs`, `src/http/pacticipants.rs`
- Create: `tests/pacticipants.rs`

**Step 1: Probe.** Against the Ruby broker, create a pacticipant and capture its document:

```bash
B=http://localhost:9292
curl -s -X PUT -H 'content-type: application/json' -d '{"repositoryUrl":"https://example.com/foo","mainBranch":"main"}' $B/pacticipants/Foo | python3 -m json.tool > tests/fixtures/ruby-broker/2.121.2/PUT_pacticipants_Foo.json
curl -s $B/pacticipants/Foo | python3 -m json.tool > tests/fixtures/ruby-broker/2.121.2/GET_pacticipants_Foo.json
curl -s $B/pacticipants | python3 -m json.tool > tests/fixtures/ruby-broker/2.121.2/GET_pacticipants_with_Foo.json
pact-broker describe-pacticipant --name Foo --broker-base-url $B > tests/fixtures/cli/describe-pacticipant-Foo.txt
```

**Step 2: Failing tests**: `PUT /pacticipants/Foo` returns the captured status and document; `GET /pacticipants/Foo` matches; `GET /pacticipants` matches the with-Foo fixture; `GET /pacticipants/Nope` still 404.

**Step 3: Implement** `Db::upsert_pacticipant`, `Db::find_pacticipant`, and the handlers, rendering the links the fixture shows (`pb:versions`, `pb:latest-version`, `pb:labels`, and so on; copy from the fixture).

**Step 4: Run, pass, commit**: `feat(pacticipants): create, fetch and list pacticipants`.

### Task 1.8: End-to-end harness with the real CLI

**Files:**
- Create: `e2e/run.sh`, `e2e/lib.sh`, `e2e/phase1_cli.sh`, `e2e/package.json`

**Step 1: `e2e/package.json`** pins the client:

```json
{
  "name": "pactling-e2e",
  "private": true,
  "devDependencies": {
    "@pact-foundation/pact-cli": "16.1.2",
    "@pact-foundation/pact": "13.2.0"
  }
}
```

**Step 2: `e2e/lib.sh`** builds the binary once, starts it on a free port with a temp SQLite file, exports `BROKER`, and kills it on exit. `e2e/run.sh` sources it and runs every `e2e/phase*_*.sh`, failing on the first non-zero exit.

**Step 3: `e2e/phase1_cli.sh`**, written to fail against the current build before Task 1.7's work is wired in (run it once before, confirm it fails, then after):

```bash
#!/usr/bin/env bash
set -euo pipefail
source "$(dirname "$0")/lib.sh"
PB="npx --prefix $(dirname "$0") pact-broker"

echo "describe-pacticipant on empty broker exits 1 with the 404 body"
set +e; out=$($PB describe-pacticipant --name Nope --broker-base-url "$BROKER" 2>&1); code=$?; set -e
[ "$code" = 1 ] || fail "expected exit 1, got $code"
grep -q "No pacticipant with name 'Nope' found" <<<"$out" || fail "unexpected output: $out"

echo "list-latest-pact-versions prints No data."
out=$($PB list-latest-pact-versions --broker-base-url "$BROKER")
[ "$out" = "No data." ] || fail "unexpected output: $out"

echo "list-environments exits 0"
$PB list-environments --broker-base-url "$BROKER" >/dev/null

echo "create-or-update-pacticipant then describe"
$PB create-or-update-pacticipant --name Foo --main-branch main --broker-base-url "$BROKER"
$PB describe-pacticipant --name Foo --broker-base-url "$BROKER" | grep -q "main" || fail "describe-pacticipant missing main branch"
echo "phase 1 e2e: PASS"
```

**Step 4: Run** `e2e/run.sh`. Expected: PASS. Fix the binary, never the script, when it does not. **Commit**: `test(e2e): real pact-broker CLI against the binary`.

### Task 1.9: Reproducible fixture capture and CI

**Files:**
- Create: `scripts/ruby-broker/docker-compose.yml` (the compose used in the evidence section, with `PACT_BROKER_SEED_EXAMPLE_DATA: "false"` and a Postgres healthcheck), `scripts/capture-ruby-fixtures.sh` (the curl loop that produced `tests/fixtures/ruby-broker/2.121.2/`), `.github/workflows/ci.yml`, `README.md`

**Step 1:** `scripts/capture-ruby-fixtures.sh` recreates every file under `tests/fixtures/ruby-broker/<version>/` from a fresh Ruby broker. Run it and confirm `git status` shows no fixture changes (same version, same documents). That proves the capture is reproducible.

**Step 2:** CI runs `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`, then `e2e/run.sh` on ubuntu-latest with node 22.

**Step 3:** README: what it is, how to run, which env vars match the Ruby broker's, and the fixture version it is verified against.

**Step 4: Commit**: `chore: fixture capture script and CI`.

**Phase 1 exit check:** `cargo test` green, `e2e/run.sh` green, `scripts/capture-ruby-fixtures.sh` produces no diff. Request an independent review of the diff (`agent-review code`) before pushing the branch.

---

# Phase 2: Publish and retrieve

Expand into bite-sized tasks after the capture task, using the Phase 1 task format.

### Task 2.1: Capture (done 2026-10-31)

Captured from Ruby broker 2.121.2 and pact-cli 16.1.2 (`pact-broker` CLI 1.77) into
`tests/fixtures/ruby-broker/2.121.2/phase2/` and `tests/fixtures/cli/phase2/` by the Phase 2
section of `scripts/capture-ruby-fixtures.sh`. The CLI's requests were recorded through
`e2e/proxy.js`. Every claim below is verified against the real broker unless marked.

**Clients.** pact-js 13 has no `Publisher`; JavaScript consumers publish with the
`pact-broker publish` CLI. The CLI does `GET /` then `POST /contracts/publish` with
`{pacticipantName, pacticipantVersionNumber, tags: [..], branch?, buildUrl?, contracts:
[{consumerName, providerName, specification: "pact", contentType: "application/json",
content: <base64>}]}`, `Accept: application/hal+json`. `tags` is always present (maybe
empty); `branch` only when given. It follows the index `self` href literally.

**publish-contracts responses (200 on success, even for a re-publish).** Body:
`logs` (deprecated copies of the notices with `level` and `deprecationWarning: "Replaced
by notices"`), `notices` (`{type, text}`), `_embedded.pacticipant` and `.version`, `_links`
`pb:pacticipant`, `pb:pacticipant-version`, `pb:pacticipant-version-tags` (array, may be
empty), `pb:contracts` (one per contract). Notices in order, per request:
1. `debug` "Created|Updated {name} version {v}[ with branch {b}][ and tags {t, ..}]";
   with tags but no branch: "with tags prod, test"; with neither: just "Created X version v".
2. `prompt` "  Next steps:\n    Configure the version branch to be the value of your
   repository branch." only when no branch was given.
3. Then per contract: `success` "Pact successfully published for {c} version {v} and
   provider {p}." or "... republished for ... with no content changes."; `debug` "  View the
   published pact at {href}"; `debug` "  Events detected: contract_published[,
   contract_content_changed] ({reasons})"; then the two webhook `prompt` lines (`"  Next
   steps:"`, `"    * Add Pact verification tests to the {p} build. See
   https://docs.pact.io/go/provider_verification"`, `"    * Configure separate {p} pact
   verification build and webhook to trigger it when the pact content changes. See
   https://docs.pact.io/go/webhooks"`).
Event reasons: the previous pact is looked up per tag of the new consumer version, or
once for "untagged" (consumer versions with no tags) when there are no tags. Per lookup:
no previous → "first time any pact published for this consumer with consumer version
tagged {t}" / "first time untagged pact published"; previous with different sha →
"pact content has changed since the last consumer version tagged with {t}" (assumed
wording, not captured) / "pact content has changed since previous untagged version";
when nothing changed the single reason is "pact content is the same as previous versions
with tags {tags joined ', '} and no new tags were applied" (two spaces when no tags) and
the event list is just `contract_published`. Re-publishing an existing version with the
same content gives "Events detected: contract_published" with no parenthesis.
Branches are ignored by event detection (verified: branch-only publishes say "untagged").

**publish-contracts errors (400, `{"errors": {...}}`).** Missing fields: `"is missing"`
per field; whitespace version: `"cannot be blank"`; wrong types: `"must be a string"`,
`"must be an array"`; contract problems go under `contracts` with ` (at index N)`:
`"is not valid Base64"`, `"content could not be parsed as application/json"`,
`"consumer name in contract content ('X') must match consumerName in contract params ('Y')"`,
`"consumerName ('Y') must match pacticipantName ('Z')"`, `"contentType must be one of:
application/json"`, `"specification must be one of: pact"`. Unparseable body:
`{"error": "JSON::ParserError - ..."}`. Empty `contracts` array is a Ruby 500; pactling
returns 400 `contracts: ["cannot be empty"]` (deliberate difference). Changing the content
of an existing consumer version is **409** with `notices` (an `error` notice with the long
"Cannot change the content of the pact ..." text, plus an `info` notice holding a diff
that pactling does not reproduce) and `errors.contracts` with the same text.

**Legacy `PUT /pacts/provider/{p}/consumer/{c}/version/{v}`.** 201 with the pact
document on create, 200 on an identical re-publish, 409 `{"error": "Cannot change..."}`
on different content, 400 `{"error": "JSON::ParserError - ..."}` on bad JSON, 400
`{"errors": {"consumer.name": ["name in pact 'Other' does not match name in URL path
'Consumer'."]}}` on a mismatch (case-sensitive). Missing names are filled from the URL.
`HEAD` is 405, `OPTIONS` 200 with `access-control-allow-methods: GET, PUT, DELETE, PATCH,
OPTIONS`, `DELETE` 200 with `{"_links": {"index": {..}, "pb:latest-pact-version": {..}}}`
(404 when absent), `PATCH` without a JSON content type is 415.

**Pact document.** The stored JSON with `_id` added to every interaction and message,
then `createdAt` and `_links` (self, pb:consumer, pb:consumer-version, pb:provider,
pb:pact-version, pb:latest-pact-version, pb:all-pact-versions,
pb:latest-untagged-pact-version, pb:latest-tagged-pact-version (templated),
pb:previous-distinct, pb:diff-previous-distinct, pb:diff (templated), pb:pact-webhooks,
pb:consumer-webhooks, pb:tag-prod-version (href ends `/tags/prod`, title says
'production'), pb:tag-version, pb:publish-verification-results,
pb:latest-verification-results, pb:triggered-webhooks, pb:matrix-for-consumer-version
(query string percent-encoded `q%5B%5D%5Bpacticipant%5D=..&latestby=cvpv`), curies
context=pact). Served as `application/hal+json` unless the client asks for exactly
`application/json`. The same document is served by `/latest`, `/latest/{tag}`,
`/latest-untagged`, `/pact-version/{sha}` and `/version/{v}`; `self` is always the
`/version/{v}` href.

**Hashes (verified by recomputation).** Interaction `_id` = SHA-1 of the interaction
serialised as compact JSON with object keys sorted recursively (`_id` excluded). Pact
version sha = SHA-1 of compact, key-sorted JSON of `{"interactions": [...],
"messages": [...], "pact_specification_version": "2.0.0"}` keeping only present keys, where
the arrays are sorted by their compact key-sorted JSON string and contain no `_id`;
`pact_specification_version` comes from `metadata.pactSpecification.version` (also
`metadata.pact-specification.version` or `metadata.pactSpecificationVersion`, assumed).
A pact with neither interactions nor messages hashes its whole key-sorted content.

**Versions, branches, tags.** Versions are ordered by creation (`order` column), newest
first in lists. A version document has `number`, optional `buildUrl`, `updatedAt` only
when updated, `createdAt`, `_embedded.branchVersions` (`{name, latest, _links.self}`) and
`_embedded.tags`, links self, pb:pacticipant, pb:tag (templated),
pb:latest-verification-results-where-pacticipant-is-consumer, pb:pact-versions (one per
pact publication, ordered by provider name), and on the single-resource document only:
pb:deployed-environments, pb:released-environments, pb:record-deployment,
pb:record-release (empty arrays until Phase 4), curies context=version. `latest-version`
and `latest-version/{tag}` and `branches/{b}/latest-version` return the version document
with `self.href` equal to the request URL. Pacticipant documents gain
`_embedded.latestVersion` plus a deprecated `latest-version` copy carrying `title` and
`name` "DEPRECATED - please use latestVersion" once a version exists.
`PUT /pacticipants/{p}/versions/{v}` creates (201, also the pacticipant) or updates (200,
`buildUrl`). `PUT .../versions/{v}/tags/{t}` needs a JSON content type (else 415),
creates the version if needed, 201 with `Location` or 200 when the tag exists; body
`{name, createdAt, _links: {self, version, pacticipant}}`. `PUT
.../branches/{b}/versions/{v}` likewise, body `{createdAt, _links: {self, pb:branch,
pb:version}}`. Branch document `{name, [updatedAt], createdAt, _links: {self,
pb:latest-version}}`; `/branches` is paged (`page: {number, size: 100, totalElements,
totalPages}`) and ordered by name. Deletes return 204 (version, tag, branch-version,
branch, pacticipant cascade) or the plain-JSON 404. Every unknown version, tag, branch or
pact is the **plain JSON route 404** ("The requested document was not found on this
server."); only an unknown pacticipant on `/pacticipants/{p}` is the HAL 404.

**Ruby defaults reproduced.** `auto_detect_main_branch`: the first branch named
`develop`, `main` or `master` becomes the pacticipant's `mainBranch` when it has none.
`use_first_tag_as_branch` (10 s window): a tag put on a version that has no branch and was
created less than 10 s earlier also creates a branch of that name (so
`publish --tag prod` without `--branch` yields branch `prod`).

**Lists.** `/pacts/latest`: `pacts` (top-level, not `_embedded`) ordered by consumer then
provider name, each `{createdAt, _embedded: {consumer: {name, _embedded.version,
_links.self}, provider: {name, _links.self}}, _links: {self: [latest href, version href]}}`;
links `pb:pacts` and deprecated `pacts`, curies context=pacts. `/pacts/provider/{p}/...`
lists have only `_links` (`self`, `pb:provider`, `pb:pacts` with `name` = consumer,
deprecated `provider` and `pacts`). `/pacts/provider/{p}/consumer/{c}/versions`,
`/tag/{t}`, `/branch/{b}` have `_embedded.pacts` (`{createdAt, _embedded.consumerVersion,
_links.self}`) and links `consumer`/`provider`/`pact-versions` (the tag variant uses
`pb:`-prefixed names). `/integrations` entries carry a relative `pb:matrix` href.

### Task 2.2: Migration `0002_versions_and_pacts.sql`

Tables `versions` (pacticipant_id, number, build_url, created_at, updated_at, UNIQUE
pacticipant+number; `id` doubles as order), `branches` (pacticipant_id, name, created_at,
updated_at), `branch_versions` (branch_id, version_id, created_at, UNIQUE), `tags`
(version_id, name, created_at, UNIQUE), `pact_versions` (consumer_id, provider_id, sha,
content, created_at, UNIQUE consumer+provider+sha), `pact_publications`
(consumer_version_id, provider_id, pact_version_id, created_at, UNIQUE
consumer_version+provider). All foreign keys `ON DELETE CASCADE`. Test: `tests/db.rs`
inserts a version and deletes the pacticipant; the version is gone.

### Task 2.3: Content hashing (`src/service/content.rs`)

Pure functions `interaction_id(&Value) -> String`, `pact_version_sha(&Value) -> String`,
`with_ids(Value) -> Value`. Unit tests assert the captured values:
`72e9b4bb90f5cb675f1cea3e426b11616e6703a0` for the first fixture interaction,
`0350e2d94a510324ee31cd7375bf1660e5d8c485` for the fixture pact,
`ed7c1aebc3e11064ce9afc29297e1e36203583cd` for `{"interactions": []}`,
`1d9541ef143e4acb6bad03910cd7cf2864b71473` for the three-interaction fixture.

### Task 2.4: Versions, branches and tags (service + db + handlers)

`service::versions`: `find_or_create_version`, `set_branch`, `add_tag` (with the 10 s
first-tag-as-branch rule and main-branch auto-detection), deletes. Routes:
`/pacticipants/{p}/versions`, `/versions/{v}` (GET, PUT, DELETE, OPTIONS),
`/versions/{v}/tags/{t}` (GET, PUT, DELETE, OPTIONS), `/branches`, `/branches/{b}` (GET,
DELETE), `/branches/{b}/versions/{v}` (GET, PUT, DELETE), `/branches/{b}/latest-version`,
`/latest-version`, `/latest-version/{t}`. Fidelity tests compare against the phase2
fixtures with hosts rewritten and timestamps stripped. Pacticipant documents embed the
latest version.

### Task 2.5: Publish (`service::publish`)

`publish_contracts(db, request) -> Result<PublishOutcome>` and
`publish_pact(db, consumer, provider, version, content) -> Result<(PactDocument, created)>`
share one `store_pact` path: find/create pacticipants (display name defaulted as in
Phase 1), version, branch, tags, pact_version by sha, pact_publication; 409 when the
publication exists with another sha. `PublishOutcome` carries what the notices need
(created vs updated version, per-contract created/republished, events with reasons).
Handlers `POST /contracts/publish` and `PUT /pacts/provider/{p}/consumer/{c}/version/{v}`
render the captured shapes; validation errors are collected into the `errors` object with
the exact strings above. Unit tests cover every validation string and every event reason.

### Task 2.6: Retrieval

Pact document rendering (`src/http/pacts.rs`): by version, latest, latest/{tag},
latest-untagged, pact-version/{sha}, DELETE; the `/pacts/latest` list; provider lists
(`/pacts/provider/{p}`, `/latest`, `/latest/{tag}`, `/tag/{tag}`, `/branch`,
`/branch/{b}`, `/branch/latest`, `/branch/{b}/latest`) and consumer-provider lists
(`/versions`, `/tag/{t}`, `/branch/{b}`, `/branch/{b}/latest/versions`,
`/branch/latest/versions`, `/branch/versions`); `/integrations`. Each compared to its
fixture.

### Task 2.7: E2E `e2e/phase2_publish.sh` and `e2e/consumer.test.js`

A real pact-js 13 consumer test writes a pact into `e2e/pacts/`; `pact-broker publish`
sends it with a branch and a tag; `list-latest-pact-versions`, `describe-version` and
`create-version-tag` outputs match `tests/fixtures/cli/phase2/` (timestamps stripped);
a second publish of changed content for the same version exits 1 with the 409 message.

# Phase 3: Verification

### Task 3.1: Capture (done 2026-10-31)

Recorded by the Phase 3 section of `scripts/capture-ruby-fixtures.sh` into
`tests/fixtures/ruby-broker/2.121.2/phase3/` and `tests/fixtures/cli/phase3/` (pact-js 13
`Verifier` traffic through `e2e/proxy.js`, driven by `e2e/provider.test.js`). Verified
unless marked.

**What the verifier sends.** `GET /`, `GET /pacts/provider/{p}/for-verification` (a probe;
200 with the beta warning notice), then `POST /pacts/provider/{p}/for-verification` with
`{"includePendingStatus": true, "consumerVersionSelectors": [...], "providerVersionBranch":
"main"}`. For each returned pact it `GET`s the pact's `_links.self.href`, which is
`.../pact-version/{sha}/metadata/{m}`, then `PUT /pacticipants/{p}/branches/{b}/versions/{v}`
(creating its own provider version and branch) and `POST
.../pact-version/{sha}/metadata/{m}/verification-results` with `{providerApplicationVersion,
success, testResults: [{interactionId, success, [interactionDescription, mismatches]}],
verifiedBy: {clientLanguage: {name, testFramework, version}, implementation, version}}`.

**Metadata.** `m` is url-safe base64 without padding of a query string: for-verification
links carry `s[][b]=main&s[][l]=true&s[][cv]=<consumer version id>&...&p=true` (one `s[]`
group per selector that matched, `p` = pending), latest-pact links carry `cv=<id>`, and
verification-result `pb:pact-version` links carry `cvn=<consumer version number>`. The
broker only needs to round-trip it: the pact fetched through a metadata URL puts the same
`m` into its `pb:publish-verification-results` href.

**for-verification response.** `{"_embedded": {"pacts": [{shortDescription,
verificationProperties: {pending, notices: [{when, text}]}, _links: {self: {href, name:
"Pact between C (v) and P"}}]}, "_links": {"self": {href, title: "Pacts to be verified"}}}`.
`pending` is present only when `includePendingStatus` is true. Short descriptions, joined
with ", " when several selectors match one pact: "latest from main branch", "latest from
branch {b}", "latest", "one of {tag}" (tag without latest), "latest with tag {tag}". The
first notice (`before_verification`) reads "The pact at {pact-version href} is being
verified because the pact content belongs to the consumer version(s) matching the
following criteri(on|a):\n    * latest version from the main branch '{b}' ({v})" with one
bullet per matched selector: "latest version from branch '{b}' ({v})", "latest version
({v})", "version tagged '{tag}' ({v})" (assumed wording for tag), "latest version with tag
'{tag}' ({v})" (assumed). With no body or no selectors the default is `mainBranch` and the
bullet reads "latest version of {consumer} from the main branch 'main' ({v})". Pending
notices (second `before_verification` plus four `after_verification:*`) are the fixed
texts in the fixtures, parameterised by provider name and branch.

**Selectors.** `mainBranch` (each consumer's main branch, latest), `branch` (+ optional
`consumer`), `matchingBranch` (needs `providerVersionBranch`, else 400 "the
providerVersionBranch must be specified when the selector matchingBranch=true is used (at
index 0)"), `tag` (all versions with the tag unless `latest: true`), `latest`, `deployed`,
`released`, `deployedOrReleased` (empty until Phase 4). A selector with none of these:
400 "must specify a value for environment or tag or branch, or specify mainBranch=true,
matchingBranch=true, latest=true, deployed=true, released=true or deployedOrReleased=true
(at index N)". Wrong types: `consumerVersionSelectors: ["must be an array"]`,
`includePendingStatus: ["must be one of: true, false"]`. Unknown provider: HAL 404 "No
provider with name 'X' found". One pact content matched by several selectors appears once.

**Pending rule (verified by four probes).** Given `providerVersionBranch` B: if B has
provider versions, pending = no successful verification of this pact content by a
provider version on B; if B has no versions, or no branch was given, pending = no
successful verification by any provider version. With `providerVersionTags`, pending = no
successful verification by a provider version carrying one of the tags (no fallback).

**Verification results.** `POST` returns 201 with `Location` and `{providerName,
providerApplicationVersion, success, verificationDate, [testResults], verifiedBy:
{implementation, version, client_implementation, client_test_framework, client_version}
(or {} when absent), _links: {self: {title: "Verification result", name: "Verification
result {n} for Pact between C (v) and P", href: .../verification-results/{n}},
pb:pact-version: {title: "Pact", name, href: .../pact-version/{sha}/metadata/{cvn}},
pb:triggered-webhooks}}`. The provider version is created when missing (no branch).
Errors: `success: ["is missing"]` / `["must be boolean"]`, `providerApplicationVersion:
["is missing"]` / `["must be filled"]`, bad JSON `{"error": "JSON::ParserError - ..."}`,
unknown pact version the plain 404. Reads: `.../verification-results/{n}`,
`.../verification-results/latest` (also under `/version/{v}/`), and
`/verification-results/consumer/{c}/version/{v}/latest` = `{success, providerSummary:
{successful, failed, unknown}, _embedded: {verificationResults: [...]}, _links: {self:
{href, title: "Latest verification results for consumer C version v"}}}`. OPTIONS:
for-verification `GET, POST, OPTIONS`; results collection `POST, OPTIONS`; a result and
latest `GET, OPTIONS, DELETE`.

### Task 3.2: Migration `0003_verifications.sql`

`verifications` (pact_version_id, provider_version_id, consumer_version_id, number,
success, test_results JSON, verified_by JSON, build_url, created_at). `number` is a global
counter (the Ruby broker's ids). Cascade from pact_versions and versions.

### Task 3.3: Selector resolution (`service::verification::select`)

Parses the request into typed selectors with the exact error strings; resolves each to
consumer versions; collects per pact publication the matching selectors; de-duplicates by
pact content. Unit tests per selector kind and for the de-duplication.

### Task 3.4: Pending and notices

`pending(db, pact_version, provider_branch, provider_tags)` per the rule above with a
unit test for each branch of it; notice rendering compared with the fixtures.

### Task 3.5: Publish and read verification results

POST with validation, provider version creation, metadata round-trip; the four read
routes; `/pacts/.../pact-version/{sha}/metadata/{m}` serving the pact document.

### Task 3.6: E2E `e2e/phase3_verify.sh`

`e2e/provider.test.js` against pactling: verification passes, results publish, the second
run reports the pact no longer pending, a failing provider publishes a failure.

# Phase 4: Environments, deployments and can-i-deploy

### Task 4.1: Capture

Against the Ruby broker with Phase 2 and 3 data: `create-environment`, `list-environments`, `record-deployment`, `record-undeployment`, `record-release`, `record-support-ended`, `can-i-deploy --to-environment` (yes and no cases, and the `--to tag` form), plus `GET /matrix?q[][pacticipant]=...` as the CLI sends it, and `GET /environments/{uuid}/deployed-versions/currently-deployed`. Save the CLI's stdout for the yes and no cases, since the reason strings are part of the contract users read.

### Task 4.2: Migrations for `environments`, `deployed_versions`, `released_versions`, and the `matrix` view

### Task 4.3: Environment CRUD and deployment recording

Includes the rule that recording a deployment marks the previous deployment of the same pacticipant to the same environment as undeployed.

### Task 4.4: Matrix query

Service `matrix(db, selectors, options) -> Matrix` with unit tests for: one integration verified, one unverified, verification of older content does not count for newer content, `latestby=cvp` and `cvpv`, environment filter, tag filter.

### Task 4.5: can-i-deploy verdict and reasons

Service `can_i_deploy(matrix) -> Verdict` with unit tests for each reason string captured in 4.1. Mutation-check every guard.

### Task 4.6: E2E `e2e/phase4_can_i_deploy.sh`

Record-deployment of the provider to `test`; can-i-deploy consumer to `test` says yes; publish a changed consumer pact; can-i-deploy says no with the captured reason; provider verifies the new pact; can-i-deploy says yes.

---

# Phase 5: Webhooks

### Task 5.1: Capture

Create a `contract_content_changed` webhook on the Ruby broker pointing at a local receiver (`e2e/receiver.js`, logs method, headers, body to a file). Publish a pact. Capture the webhook CRUD responses, the test-execution endpoint response, the execution log document, and the request the broker sent, including the `${pactbroker.*}` template expansion.

### Task 5.2: Migrations for `webhooks`, `webhook_executions`, `webhook_deliveries` (the retry queue)

### Task 5.3: Webhook CRUD and template rendering

Unit tests: every template variable from the capture, event filtering by consumer and provider.

### Task 5.4: Event selection

Service `events_for_publish(before, after) -> Vec<Event>`: `contract_content_changed` only when the sha differs; `contract_requiring_verification_published` when any selector-matching provider version lacks a verification. Unit tests for both and for the no-change case.

### Task 5.5: Delivery queue

A tokio task that polls `webhook_deliveries` for due rows, sends with reqwest, records an execution, and reschedules with the Ruby broker's default backoff (10s, 60s, 120s, 300s, 600s, 1200s; confirm from its docs or source and cite the source in the commit). Restart-safety test: enqueue, drop the task, start a new one, delivery still happens.

### Task 5.6: E2E `e2e/phase5_webhook.sh`

Receiver starts, webhook created via CLI `create-webhook`, publish, receiver file contains the expected body within 10 seconds.

---

# Phase 6: Release

### Task 6.1: `Dockerfile` (multi-stage, distroless, `/data` volume) and `docker-compose.yml` that is a drop-in for `pact-broker-infra`'s
### Task 6.2: Homebrew formula in `homebrew/pactling.rb`, following `pomo-tui/homebrew`
### Task 6.3: README with the compose swap, env var table, and "verified against Ruby broker 2.121.2 and pact-cli 16.1.2"
### Task 6.4: GitHub release workflow building macOS arm64 and linux x86_64 binaries

---

## Definition of done checklist

- [ ] `cargo test` green
- [ ] `e2e/run.sh` green on CI for phases 1 through 5
- [ ] `scripts/capture-ruby-fixtures.sh` reproduces every fixture without diff
- [ ] Independent review of each phase's diff completed before push
- [ ] README documents the compose swap and verified versions
