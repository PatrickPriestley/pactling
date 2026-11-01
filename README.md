# pactling

A [Pact](https://pact.io) broker in one binary, with an embedded SQLite database.

The goal is a drop-in replacement for the Ruby Pact broker: pact-js, the `pact-broker` CLI
and the Pact docker CLI work against it unchanged. Behaviour is verified two ways, both
in CI:

- **Fidelity tests** replay captured Ruby broker responses (`tests/fixtures/ruby-broker/`)
  against pactling and diff the documents.
- **End-to-end tests** (`e2e/`) run the real `pact-broker` CLI and pact-js against the
  binary.

Compatibility is verified against Ruby broker 2.121.2 and pact-cli 16.1.2.

## Status

Phases 1 to 5 of the [implementation plan](docs/plans/2026-10-30-pact-broker-implementation.md):
HAL index, basic auth, pacticipants, versions, branches, tags, publishing (both
`pb:publish-contracts` and the legacy `PUT`), every pact retrieval document,
pacts-for-verification with consumer version selectors and pending, verification
results, environments, deployments and releases, the matrix and can-i-deploy, and
webhooks (every event, template parameter, test execution, the delivery queue with the
Ruby broker's schedule and retries, execution logs and status). A pact-js 13 consumer
test, the `pact-broker` CLI (publish, environments, deployments, can-i-deploy, webhooks)
and a pact-js 13 provider verification run against the binary in `e2e/`, with a local
webhook receiver checking what the broker sends. Every document, verdict and notice is
replayed from Ruby broker 2.121.2 captures under `tests/fixtures/`. Phase 6 (release
packaging) follows. See the [design](docs/plans/2026-10-30-pact-broker-design.md).

Known differences from the Ruby broker, all deliberate: an empty `contracts` array is a 400
rather than a 500; the 409 for changed content omits the Ruby broker's diff notice; bad-JSON
error messages use serde's wording; webhook requests carry `User-Agent: Pactling/<version>`
rather than `Pact Broker v2.121.2`; the webhook host whitelist takes host names and
`/regex/` entries but not CIDR ranges; label-scoped webhooks are stored but never fire
(pactling has no labels); `/pacts/provider/{p}/consumer/{c}/latest/verification-results/latest`
is not served (use the `version/{v}` form).

The read-only extension `GET /pactling/v1/can-i-deploy/explanation` accepts matrix
selectors plus a target environment or tag and explains the shared verdict with evidence
and suggested next actions.
See the [deployment explanation API](docs/api/deployment-explanations.md) for its versioned
schema, open reason/action codes and snapshot limits. Existing Ruby-compatible routes and
documents are unchanged.

The read-only HTML UI is another deliberate extension: open `/ui/` on the same server
for integrations, exact pacts, pair matrices and deployment explanations. It needs no
JavaScript and uses the broker's existing read authentication. Unlike Ruby's UI, it has
its own namespace; `/` and existing HAL/JSON responses stay unchanged. See the
[minimal UI guide](docs/api/minimal-ui.md).

Webhook settings use the Ruby broker's variable names and defaults:
`PACT_BROKER_WEBHOOK_SCHEME_WHITELIST` (default `https`), `PACT_BROKER_WEBHOOK_HOST_WHITELIST`
(empty: any host, response details hidden from logs), `PACT_BROKER_WEBHOOK_HTTP_METHOD_WHITELIST`
(`POST`), `PACT_BROKER_WEBHOOK_HTTP_CODE_SUCCESS` (`200 201 202 203 204 205 206`) and
`PACT_BROKER_WEBHOOK_RETRY_SCHEDULE` (`10 60 120 300 600 1200` seconds); lists are
space-separated. `PACTLING_WEBHOOK_INITIAL_DELAY` and `PACTLING_WEBHOOK_DELAY_STEP` (5 and 3
seconds, the Ruby broker's `5 + 3i`) set when the i-th webhook fired by one request runs.

## Run

Release artifacts are published by a `v<version>` tag matching `Cargo.toml`.
The first release and tap publication are still pending; until then use
`docker compose up --build -d` from this checkout or build with Cargo below.

Before distributing the first image, the maintainer must set the GHCR package's
visibility to **Public** in its package settings; GitHub defaults new packages to
private ([registry documentation](https://docs.github.com/en/packages/working-with-a-github-packages-registry/working-with-the-container-registry)).
Stable releases update `latest`; prerelease tags publish only their versioned image.

After a release, run the container with persistent storage:

```bash
docker run -d --name pact-broker -p 9292:9292 -v pact-data:/data \
  ghcr.io/patrickpriestley/pactling:latest
# Or use this repository's docker-compose.yml:
docker compose pull
docker compose up -d --no-build
```

To swap a Ruby broker Compose service, keep its service name, port mapping and auth /
webhook settings, change the image, remove `PACT_BROKER_DATABASE_*` settings and the
Postgres dependency, and add the SQLite URL and volume:

```yaml
services:
  pact-broker:
    image: ghcr.io/patrickpriestley/pactling:latest
    ports: ["9292:9292"]
    environment:
      PACTLING_DATABASE_URL: sqlite:///data/pactling.db?mode=rwc
      # Keep your PACT_BROKER_BASIC_AUTH_* and webhook settings here.
    volumes: ["pact-data:/data"]
volumes:
  pact-data:
```

This starts a new broker; it does not import an existing Postgres database. The image
runs as UID/GID 65532, and a fresh named volume is writable by that user. Host bind
mounts must provide equivalent permissions. Pin an image tag such as `v0.1.1` when
upgrading deliberately.

The release includes macOS arm64 and Linux x86_64 (glibc 2.36+) tarballs and `.sha256`
files. Verify with `shasum -a 256 -c <archive>.sha256`, extract, and put `pactling`
on your PATH. The image supports Linux arm64 and x86_64.

The release also includes `pactling.rb`, generated from those exact tarballs. Copy it
to `Formula/pactling.rb` in the existing `PatrickPriestley/homebrew-tap`; once published:

```bash
brew install PatrickPriestley/tap/pactling
pactling --version
```

The checked-in `homebrew/pactling.rb` records local build checksums for packaging
validation. Use the formula attached to the release for distribution: rebuilds may
have different checksums. See [local packaging evidence](docs/plans/2026-11-01-phase6-release.md).

Build from source:

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
| `PACT_BROKER_WEBHOOK_SCHEME_WHITELIST` | Allowed schemes (default `https`) |
| `PACT_BROKER_WEBHOOK_HOST_WHITELIST` | Hosts or `/regex/` entries (empty: any host; response details hidden) |
| `PACT_BROKER_WEBHOOK_HTTP_METHOD_WHITELIST` | Allowed methods (default `POST`) |
| `PACT_BROKER_WEBHOOK_HTTP_CODE_SUCCESS` | Success codes (default `200 201 202 203 204 205 206`) |
| `PACT_BROKER_WEBHOOK_RETRY_SCHEDULE` | Retry delays in seconds (default `10 60 120 300 600 1200`) |
| `PACTLING_WEBHOOK_INITIAL_DELAY` / `PACTLING_WEBHOOK_DELAY_STEP` | Initial and per-delivery delay in seconds (defaults 5 / 3) |

With no credentials configured the broker is open, as the Ruby broker is.

## Explain a deployment

`pactling explain` queries the explanation API and prints the decision and one concise line per reason, including the involved pair,
suggested action target and a useful evidence link:

```bash
PACT_BROKER_BASE_URL=http://localhost:9292 pactling explain \
  --pacticipant Consumer --version 1.0.0 --to-environment production

pactling explain --pacticipant Consumer --main-branch --to-environment test \
  --broker-base-url http://localhost:9292 --output json
```

Choose one selector: `--version V`, `--latest [TAG]`, `--branch B` or `--main-branch`.
Choose one target: `--to-environment ENV` or `--to TAG`. Repeat `--ignore NAME[=VERSION]`
to exclude specific participants or versions using the broker's existing semantics.
`--output text` is the default; `--output json` preserves the endpoint document bytes.

Exit codes are **0 allowed, 1 blocked, 2 unknown, 3 usage/request error**. Check the exit
code even in JSON mode; unknown never means success. Client configuration uses
`PACT_BROKER_BASE_URL`, `PACT_BROKER_USERNAME`/`PACT_BROKER_PASSWORD`, or
`PACT_BROKER_TOKEN`, with matching `--broker-*` options. Basic and bearer credentials
are mutually exclusive. See the [CLI contract](docs/api/deployment-explanations.md#command-line-client)
for options, errors and output details.

Running `pactling` without a subcommand remains the server, with all existing server
flags and environment variables. `pactling --help` and `pactling --version` keep their
server behavior; use `pactling explain --help` for client usage.

## Develop

```bash
cargo test                 # unit and fidelity tests
python3 scripts/test-release.py # packaging contracts
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
