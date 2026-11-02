# Retry probes for issue #13

Run 2026-11-02 against fresh project `pactling-audit-probe`, pinned Ruby broker **2.121.2**
(`ruby.json` root response header), and freshly built pactling main `dc4c4cf` / 0.1.2.
These are **real-system observations**, not mock acceptance. Only documentation/evidence
is committed; disposable drivers/databases remain under `/tmp/pactling-audit-probe`.

```sh
docker compose -p pactling-audit-probe -f scripts/ruby-broker/docker-compose.yml up -d
CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0 cargo build -j 2
PACT_BROKER_PORT=9394 PACTLING_DATABASE_URL=sqlite:///tmp/pactling-audit-probe/pactling.db?mode=rwc target/debug/pactling
python3 /tmp/pactling-audit-probe/retries.py http://localhost:9292 /tmp/pactling-audit-probe/ruby.json
python3 /tmp/pactling-audit-probe/retries.py http://localhost:9394 /tmp/pactling-audit-probe/pactling.json
```

`ruby.json` and `pactling.json` record every request's method, actual URL, body, selected
headers, status, Location and full response in execution order. The scratch driver uses
Python urllib, not a fake receiver; each later UUID/link is taken from the prior response.
It sleeps 1.1 seconds between identical environment/webhook updates to expose timestamp
changes. These files supply the complete request recipe without relying on the scratch
script surviving. A fresh replay must resolve links/UUIDs anew, not reuse these instance IDs.
The cases are a targeted sample, not a claim of full-document parity or concurrency coverage.

Real globally installed **pact-broker CLI 1.77.0** was recorded through the repository proxy:

```sh
node e2e/proxy.js 9393 http://localhost:9292 /tmp/pactling-audit-probe/ruby-cli.jsonl
node e2e/proxy.js 9395 http://localhost:9394 /tmp/pactling-audit-probe/pactling-cli.jsonl
# Repeat against each proxy base URL:
pact-broker record-support-ended --pacticipant AuditC --version 1 --environment audit --broker-base-url http://localhost:9393
pact-broker record-release --pacticipant AuditC --version 1 --environment audit --broker-base-url http://localhost:9393
pact-broker record-deployment --pacticipant AuditC --version 1 --environment audit --application-instance blue --broker-base-url http://localhost:9393
# Repeat deployment command, then publish the recorded pact twice:
pact-broker publish /tmp/pactling-audit-probe/pact.json --consumer-app-version 1 --branch main --tag dev --broker-base-url http://localhost:9393
```

All those CLI commands completed successfully against both brokers. `*-cli.jsonl` captures
requests/responses, including the exact `application/merge-patch+json` used for support-ended.
The direct driver initially sent application/json PATCH: Ruby returned **415**, so its
following `release-after-end-retry` did **not** establish reactivation. The subsequent real
CLI PATCH returned 200 on both, and re-release returned 200/currentlySupported:true. Do not
interpret the unsuccessful direct PATCH as a successful state transition. Pactling accepts
that direct PATCH; that existing discrepancy is evidence, not a proposed change here.

`publish-effects.json` contains the exact CLI argv/exit/output and active webhook setup.
For this follow-up, restart the same Ruby project and pactling with
`PACT_BROKER_WEBHOOK_SCHEME_WHITELIST=http`, and run
`node e2e/receiver.js 9396 /tmp/pactling-audit-probe/deliveries.jsonl`.
Webhook destinations are `http://host.docker.internal:9396/ruby` and
`http://localhost:9396/pactling`, event `contract_published`. `deliveries.jsonl` records
**two deliveries from each broker** after two unchanged publishes. The webhook-create
retry probes used disabled webhooks at example.invalid; they made no outside delivery.

`auth-statuses.json` uses a separate file-backed pactling instance on 9397, configured
Basic read/write and read-only pairs, public reads and a synthetic bearer token. It records
only identity classes/statuses, never Authorization values. Anonymous/read-only POST is401;
read/write and bearer reach body validation (400 for `{}`); all GETs are200 with public
read enabled. Initial setup incorrectly supplied client variable PACT_BROKER_TOKEN to the
server; token POST was401. Repeating with the server's actual PACTLING_BEARER_TOKEN produced
the committed result. Neither proposed audit authorization nor new mutation APIs exist yet.

Cleanup: stop only these probe listeners; run `docker compose -p pactling-audit-probe -f
scripts/ruby-broker/docker-compose.yml down -v`, then `cargo clean` in this clone. No image
build or buildx builder was needed; no owner images/volumes were pruned.

**Cleanup verified:** compose `down -v` completed; subsequent `compose ps -a` is empty.
All five owned host listeners (9393–9397) were stopped. `cargo clean` reported
`Removed 2621 files, 728.6MiB total` in `/tmp/pactling-audit-plan`.
