# pactling

A [Pact](https://pact.io) broker in one binary, with an embedded SQLite database.

The goal is a drop-in replacement for the Ruby Pact broker: pact-js, the `pact-broker` CLI
and the Pact docker CLI work against it unchanged. Behaviour is verified two ways, both
in CI:

- **Fidelity tests** replay captured Ruby broker responses (`tests/fixtures/ruby-broker/`)
  against pactling and diff the documents.
- **End-to-end tests** (`e2e/`) run the real `pact-broker` CLI and pact-js against the
  binary.

Verified against Ruby broker **2.121.2** and `@pact-foundation/pact-cli` **16.1.2**.

## Status

Phases 1 to 4 of the [implementation plan](docs/plans/2026-10-30-pact-broker-implementation.md):
HAL index, basic auth, pacticipants, versions, branches, tags, publishing (both
`pb:publish-contracts` and the legacy `PUT`), every pact retrieval document,
pacts-for-verification with consumer version selectors and pending, verification
results, environments, deployments and releases, the matrix and can-i-deploy. A pact-js 13
consumer test, the `pact-broker` CLI (publish, environments, deployments, can-i-deploy)
and a pact-js 13 provider verification run against the binary in `e2e/`. Every matrix
verdict, notice and reason string is replayed from Ruby broker 2.121.2 captures
(`tests/fixtures/ruby-broker/2.121.2/phase4/`, 73 can-i-deploy cases in
`matrix-regressions.json`). Webhooks follow. See the
[design](docs/plans/2026-10-30-pact-broker-design.md).

Known differences from the Ruby broker, all deliberate: an empty `contracts` array is a 400
rather than a 500; the 409 for changed content omits the Ruby broker's diff notice; bad-JSON
error messages use serde's wording.

## Run

```bash
cargo run -- --port 9292
# or
PACT_BROKER_PORT=9292 PACTLING_DATABASE_URL='sqlite:///data/pactling.db?mode=rwc' pactling
```

Configuration reuses the Ruby broker's variable names where one exists:

| Variable | Meaning |
| --- | --- |
| `PACT_BROKER_PORT` | Listen port (default 9292) |
| `PACTLING_DATABASE_URL` | SQLite URL (default `sqlite://pactling.db?mode=rwc`) |
| `PACT_BROKER_BASIC_AUTH_USERNAME` / `_PASSWORD` | Read/write user |
| `PACT_BROKER_BASIC_AUTH_READ_ONLY_USERNAME` / `_PASSWORD` | Read-only user |
| `PACT_BROKER_ALLOW_PUBLIC_READ` | `true` lets anonymous clients read |
| `PACTLING_BEARER_TOKEN` | Static bearer token with read/write rights (pactling only) |

With no credentials configured the broker is open, as the Ruby broker is.

## Develop

```bash
cargo test                 # unit and fidelity tests
(cd e2e && npm ci)         # once
bash e2e/run.sh            # real clients against the binary
```

To regenerate the fixtures from a fresh Ruby broker (needs Docker):

```bash
scripts/capture-ruby-fixtures.sh /tmp/recaptured
scripts/compare-fixtures.py tests/fixtures /tmp/recaptured
```

The comparison ignores timestamps and uuids; anything else that differs is a real change
in the reference broker and needs a look.
