# Pactling: a Pact broker in one binary

Design, accepted 2026-10-30.

## Goal

A drop-in replacement for the Ruby Pact broker, shipped as one binary with an embedded
SQLite database. Existing clients work unchanged: pact-js 10 and later, the standalone
`pact-broker` CLI and the Pact docker CLI. Version 1 covers the full pipeline: publish,
verify, can-i-deploy, deployments and releases, and webhooks.

Version 1 supports the modern workflow model only: branches, environments, deployed and
released versions, consumer version selectors. Tags are accepted on publish and stored,
and `can-i-deploy --to <tag>` works, but the model reasons about environments.

Out of scope for version 1: the HTML UI, badges, SSL termination, the `pb:dashboard`
relation and Pactflow-only relations. The HAL index omits their links, which is how
clients discover they are unsupported.

## Definition of done

All of these run in CI against the Rust binary, driven by the real clients:

1. A pact-js 13 consumer publishes a contract with a branch via `pb:publish-contracts`.
2. The pact-js provider fetches it through `pacts-for-verification` with consumer version
   selectors, verifies, and publishes results.
3. The standalone `pact-broker` CLI runs `record-deployment` and `can-i-deploy`, and the
   verdict flips from yes to no after a breaking contract change.
4. A `contract_content_changed` webhook fires at a test receiver that asserts on the body.
5. The legacy `pact-broker publish` command (PUT by consumer version) still works, because
   older pipelines depend on it.

## Architecture

One binary, `pactling`. Run it with a data directory and a port; it creates the SQLite
file and runs migrations on first start. Configuration comes from flags and environment
variables that reuse the Ruby broker's names where they exist, such as
`PACT_BROKER_BASIC_AUTH_USERNAME`, so existing compose files and CI secrets carry over.

Crates: axum (HTTP), sqlx with SQLite (storage and migrations), serde_json (pact
documents), reqwest (webhook delivery), tokio (webhook queue), clap (CLI). No ORM. Every
query is plain SQL in one `db` module, so a Postgres feature later touches only that module.

Three layers:

- **HTTP.** Axum handlers parse the request, call one service function and render HAL
  JSON. Every handler emits its own link relations. Fidelity to the Ruby broker lives here
  and the layer stays thin so fidelity tests target it directly.
- **Service.** Pure functions over domain types: publish contract, record verification,
  compute matrix, evaluate can-i-deploy, select webhooks to fire. They take a database
  handle and return domain results. No HTTP types.
- **Storage.** SQL and migrations, shipped inside the binary.

The pact document is stored as the raw JSON blob plus a content hash. The broker never
inspects interactions; consumer, provider and hash are enough, and the hash is what gives
the Ruby broker's "pact version" identity.

## Data model

Tables mirror the Ruby broker closely enough that matrix semantics are reproducible:

- `pacticipants`, `versions` (number, branch, pacticipant), `tags`
- `branches`, `branch_versions`, `environments`, `deployed_versions`, `released_versions`
- `pact_publications` (consumer version, provider, pact version), `pact_versions`
  (content, sha)
- `verifications` (provider version, pact version, success, build URL)
- `webhooks`, `webhook_executions`
- a `matrix` SQL view joining publications to verifications, not cached

can-i-deploy is one query over the matrix view plus the environment tables, with the Ruby
broker's rule: a version is deployable to an environment if every integration it has with
what is currently deployed there has a successful verification of the exact pact content.

## API surface

Everything hangs off the HAL index at `/`. Clients read it once and follow relations, so
the index must match the Ruby broker's relation names and URL templates exactly.

- **Publish.** `pb:publish-contracts` (POST, pacticipant, version, branch, tags and the
  contracts in one body). Also legacy
  `PUT /pacts/provider/{p}/consumer/{c}/version/{v}` for the CLI's `publish` command.
- **Retrieve.** GET a specific pact, latest pact per integration, and
  `pacts-for-verification` (POST with consumer version selectors). Selectors in v1:
  mainBranch, matchingBranch, deployedOrReleased, deployed, released, branch, tag, latest.
- **Verification results.** POST under the pact's `pb:publish-verification-results`
  relation, storing provider version and branch.
- **Deployment.** Environments CRUD, record-deployment, record-undeployment,
  record-release, record-support-ended.
- **Matrix and can-i-deploy.** `GET /matrix` with the CLI's query grammar, including
  `--to-environment` and `--to <tag>`. Output includes the reason strings the CLI prints.
- **Webhooks.** CRUD; events `contract_content_changed`,
  `contract_requiring_verification_published`, `provider_verification_published`; body
  template variables; test-execution endpoint; execution logs. Delivery is a tokio task
  with a bounded retry schedule persisted in SQLite, so restarts do not lose deliveries.
- **Pacticipants, versions, branches, tags.** Enough CRUD for the CLI's
  create-or-update-pacticipant and describe-version commands.

## Authentication

HTTP basic auth with a read/write pair and an optional read-only pair, plus a static
bearer token. This covers every option pact-js and the CLI expose. When no credentials are
configured the broker is open, matching the Ruby broker's default.

## Error handling

Follow the Ruby broker's conventions because clients parse them: 404 with a HAL error
body for unknown resources; 400 with `errors` keyed by field for malformed pacts or
selectors; 409 when republishing different content for an existing consumer version
without a branch; 401 with `WWW-Authenticate: Basic`. Errors are one enum in the service
layer mapped to HTTP in one place. Webhook delivery failures are recorded in executions
and never surface as request errors.

## Testing

- **Unit tests** on the service layer with an in-memory SQLite per test: matrix rows,
  can-i-deploy verdicts, selector resolution, webhook event selection. Every guard in
  can-i-deploy is mutation-checked.
- **Fidelity tests** on the HTTP layer. A script in the repo captures responses from the
  real Ruby broker (the `pact-broker-infra` compose stack) into JSON fixtures. Tests
  replay the same requests against the Rust binary and diff the HAL bodies, ignoring
  timestamps and hosts. The fixtures are the evidence that the API shape is right.
- **End-to-end tests** in `e2e/` run the real clients against the binary in CI: pact-js
  13 consumer and provider, the standalone CLI in Docker, and a webhook receiver. These
  are the acceptance tests in "Definition of done".

## Build order

Each phase ends with an end-to-end test that was red before it.

1. **Skeleton.** Binary, config, migrations, auth, HAL index matching the Ruby fixture.
   E2E: CLI `describe-pacticipant` against an empty broker returns a correct 404.
2. **Publish and retrieve.** Both publish endpoints, pacticipants, versions, branches,
   tags, latest pact. E2E: pact-js consumer publishes, CLI lists it.
3. **Verification.** pacts-for-verification with selectors, publish results. E2E: pact-js
   provider verifies and publishes.
4. **Environments and can-i-deploy.** Environment CRUD, deployments, releases, matrix,
   verdict reasons. E2E: can-i-deploy says yes, then no after a breaking change.
5. **Webhooks.** CRUD, delivery queue, retries, execution log. E2E: webhook fires on publish.
6. **Release.** Dockerfile, Homebrew formula, README with the compose swap from
   `pact-broker-infra`.

## Decisions and reasons

- **SQLite via sqlx, plain SQL.** One file, no server, and the matrix is naturally a SQL
  problem. Keeping SQL in one module leaves Postgres open.
- **Raw pact JSON plus hash.** The broker's job is identity and routing, not
  interpretation. Avoids coupling to pact specification versions.
- **Fixtures from the real broker, not from docs.** Clients follow HAL links by name;
  documentation has been wrong about exact shapes before. Captured responses are the
  only evidence that counts.
- **Real clients in CI.** A mock client proves only our side. The acceptance tests use
  pact-js and the official CLI unchanged.
