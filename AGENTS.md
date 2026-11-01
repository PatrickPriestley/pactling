# Working on pactling with an agent

pactling is a drop-in replacement for the Ruby Pact broker. The only source of truth for
behaviour is the real Ruby broker 2.121.2 and the real clients; this file tells an agent
how to get at that evidence quickly.

## Ground rules

- **Fixtures are the evidence.** Every response shape, status code, error string and
  ordering comes from `tests/fixtures/ruby-broker/2.121.2/` (HTTP) and
  `tests/fixtures/cli/` (CLI output, proxied client traffic as `.jsonl`). Docs and memory
  have been wrong; fixtures have not. Before implementing an endpoint, find or capture its
  fixture.
- **Probe before you assert.** `scripts/ruby-broker/docker-compose.yml` starts the
  reference broker on `:9292` (image pinned by digest). Add probes to the matching phase
  section of `scripts/capture-ruby-fixtures.sh`, re-run it, and commit the new fixtures.
  `scripts/compare-fixtures.py tests/fixtures <recapture>` must report zero differences
  on a recapture.
- **Record what real clients send** with `e2e/proxy.js <port> <upstream> <log.jsonl>`.
  `e2e/consumer.test.js` (pact-js consumer) and `e2e/provider.test.js` (pact-js provider
  verifier, configured by environment) are the client drivers; the `pact-broker` CLI is
  `e2e/node_modules/.bin/pact-broker`.
- **Deliberate differences** from the Ruby broker are listed in `README.md`. Add to that
  list rather than silently diverging.
- **Webhook captures** (phase 5) start two `e2e/receiver.js` receivers on the host and let
  the broker call them over plain http (`PACT_BROKER_WEBHOOK_SCHEME_WHITELIST=http` in the
  probe compose). The broker reaches the host as `host.docker.internal`; on Linux map it
  with `extra_hosts`. Deliveries are scheduled 5 + 3i seconds out, so the capture waits
  30 s before reading triggered webhooks; `tests/fixtures/cli/phase5/receiver*.jsonl` is
  what the Ruby broker actually sent.

## Layout

- `src/http/` renders HAL documents and owns every link relation. One module per
  resource family; handlers match on `Method` and answer 405 with `allow`, OPTIONS with
  `access-control-allow-methods`.
- `src/service/` holds the rules (publish, selectors, pending, event detection). No HTTP
  types.
- `src/db/` holds all SQL (sqlx 0.9 wants static SQL strings; the `publication_query!`
  macro builds the common SELECT). Migrations live in `migrations/`; add a file and
  `build.rs` makes cargo notice it.
- `tests/*.rs` are fidelity tests: replay a capture sequence against an in-memory
  server and compare documents with `fixture()` + `stable()` (timestamps stripped,
  hosts rewritten). `e2e/phase*_*.sh` run the real clients against the built binary.

## Ruby quirks you must keep

- POST `/pacticipants` answers `Location: .../pacticpants/<name>` (sic).
- Missing versions, tags, branches and pacts are the plain-JSON route 404; missing
  pacticipants on `/pacticipants/{p}` and on the versions/branches lists are the HAL 404.
- A tag put within 10 s of a version's creation also creates a branch; the first branch
  named develop, main or master becomes the pacticipant's main branch.
- A branch head is the last version added to the branch, not the highest version.
- Pact version sha and interaction ids are SHA-1 over key-sorted compact JSON
  (`src/service/content.rs`).
- `pending` is always reported; the rule is in `src/service/verification.rs`.
- Verification result links carry base64 metadata; only `cv=` is read back.
- Webhook uuids are 22 url-safe base64 characters; a missing body is sent as the string
  `null`; without a content-type header the request is `application/x-www-form-urlencoded`.
- Publish notices: every trigger is listed; with webhooks but none for the detected
  events, "No enabled webhooks found for the detected events"; the "Add Pact verification
  tests" step appears until the integration has a verification, the webhook step until a
  webhook covers the pact.

## Verifying a change

```
cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
(cd e2e && npm ci) && bash e2e/run.sh
```

Then mutation-check each guard the change adds (break it, see a test fail, restore it)
and run `agent-review code <base>..HEAD --goal "..."` before pushing.
