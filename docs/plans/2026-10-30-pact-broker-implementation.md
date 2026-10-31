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

### Task 2.1: Capture (probe first)

Against the Ruby broker, with `tests/fixtures/pact/consumer-provider.json` (a minimal pact v3 document, two interactions):

1. `pact-broker publish tests/fixtures/pact --consumer-app-version 1.0.0 --branch main --tag dev` with `--verbose`; record every request the CLI makes (method, path, headers, body) and every response. The CLI uses `pb:publish-contracts` when the index advertises it.
2. The legacy route: `curl -X PUT .../pacts/provider/Provider/consumer/Consumer/version/1.0.0` with the pact body; capture 201 and the 200 on a re-publish of identical content, and the 409 on different content for the same version.
3. `GET` the pact by version, the latest pact, `pb:latest-pact-versions`, the pacticipant's versions list, the version document, a branch document, a tag document.
4. pact-js 13 consumer test with `@pact-foundation/pact`'s `Publisher`, run against the Ruby broker; record its requests with a logging proxy (`mitmproxy` or a 20-line node http proxy in `e2e/proxy.js`).

### Task 2.2: Migrations for versions, branches, branch_versions, tags, pact_versions, pact_publications

### Task 2.3: `pb:publish-contracts`

Service function `publish_contracts(db, request) -> PublishResult` with unit tests for: new pacticipant created, version created with branch, same content re-published is idempotent, different content for same version without branch is a conflict, tags applied. Handler renders the captured response including its `notices` array and links.

### Task 2.4: Legacy `PUT` publish

### Task 2.5: Pact retrieval endpoints and version, branch and tag documents

### Task 2.6: E2E `e2e/phase2_publish.sh` and `e2e/phase2_publish.test.js`

CLI publish, pact-js `Publisher`, then `list-latest-pact-versions` shows the integration and `describe-version` shows branch and tag.

---

# Phase 3: Verification

### Task 3.1: Capture

Run pact-js 13 provider verification against the Ruby broker with `consumerVersionSelectors: [{ mainBranch: true }, { deployedOrReleased: true }, { matchingBranch: true }]` and `publishVerificationResult: true`, behind the logging proxy. Record the `for-verification` POST body and response (including `verificationProperties` with `pending` and `notices`), and the verification-results POST and response.

### Task 3.2: Migration for `verifications`

### Task 3.3: Selector resolution

Service `resolve_selectors(db, provider, selectors) -> Vec<PactToVerify>` with a unit test per selector kind: `mainBranch`, `matchingBranch`, `branch`, `tag`, `latest`, `deployed`, `released`, `deployedOrReleased`, and a de-duplication test (one pact matched by two selectors appears once, with both selector reasons). Mutation-check every branch of the match.

### Task 3.4: `for-verification` handler with `pending` semantics

Pending is true when the provider has no successful verification of that pact content from its main branch. Unit test both sides.

### Task 3.5: Publish verification results

### Task 3.6: E2E `e2e/phase3_verify.test.js`

Provider verifies and publishes; `describe-version` for the provider shows a verification; the fixtures' `notices` text matches.

---

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
