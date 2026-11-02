---
status: proposed
accepted_by: null
accepted_on: null
issue: 13
---

# Idempotent agent-facing mutations and attributable audit history

## Goal and boundary

Agents and CI can lose a response after a broker commits, then retry an operation that
creates another verification, schedules another webhook, or restores an older deployment.
[#13](https://github.com/PatrickPriestley/pactling/issues/13) needs explicit retry semantics
and attributable committed history without changing Ruby-compatible behavior. Add an
opt-in HTTP mutation facade, durable idempotency receipts, and a transactional audit trail
for the listed write families, including writes through existing routes. Reuse configured
authentication and existing business rules. This is a proposed implementation plan only.
No implementation or new MCP write tools are included in this PR.

On successful storage operations, existing routes, status codes, HAL/JSON bodies, CLI
commands, links and webhook delivery semantics remain unchanged. A deliberate new failure
condition is mandatory audit persistence: an audit failure rolls back a legacy write and
uses its existing storage-error response. This fail-closed difference must be accepted and
listed in README; byte compatibility is not a promise that new storage work cannot fail. Existing reads (including the five MCP tools, UI and explanation)
never create audit/idempotency entries or trigger cleanup/writes. Deletes, webhook test
execution, arbitrary HTTP forwarding, external identity providers, security-event logging,
and tamper-evident/compliance archives are outside this increment. Undeployment/support-ended
are context probes here, not new mutation commands; auditing those and other omitted writes
is later work. History must explicitly state this coverage, not imply a complete broker log.

## Probed facts and evidence

Base: main `dc4c4cf` (merged #39), pactling 0.1.2, pinned Ruby **2.121.2**, real Pact CLI
**1.77.0**. [Exact requests/responses, CLI traffic and receiver output](../probes/idempotent-mutations-audit/README.md)
are committed. Labels below apply only to the stated sample. All proposed contracts below
are **design decisions, not verified implementation claims**.

| Label | Observed retry behavior in both real brokers | Consequence |
| --- | --- | --- |
| **R1: verified against real systems** | Exact pact PUT:201 then200. `/contracts/publish`:200 twice. Two unchanged CLI publishes produce **two actual contract_published deliveries per broker**. | Content identity prevents duplicate publication identity, not duplicate intended effects. Agent publishing needs an operation key. |
| **R2: verified against real systems** | Identical verification POST with the same arbitrary `Idempotency-Key` header:201 twice with distinct result numbers; changed success with that same header also201/new result. | Existing routes do not implement that key contract. A genuine new verification run must remain distinguishable from a transport retry. |
| **R3: verified against real systems** | Deployment same version/environment/instance:201 twice, same UUID. Deploy v2, then repeat v1:201 with a new UUID, restoring v1. | Natural identity is conditional on current state; delayed retries need a key. |
| **R4: verified against real systems/client** | Release same version/environment:201 then200/same UUID. Real CLI support-ended followed by re-release returns200/currentlySupported:true. | Natural release identity alone does not protect against stale reactivation. The direct Ruby application/json PATCH got415; only the subsequent successful CLI merge-patch proves this transition. |
| **R5: verified against real systems** | Environment POST:201 then400 duplicate name; identical PUT to UUID:200 twice, updatedAt changes. | Name is a uniqueness rule, not successful response replay. Updates need keys to avoid restoring stale values. |
| **R6: verified against real systems** | Tag membership PUT:201 then200. Branch membership PUT:201 then200; add v2 then repeat v1 leaves latest branch version2. | Ensure-membership natural identities suffice while the membership persists; optional keys protect retries across intervening removal. |
| **R7: verified against real systems** | Webhook collection POST:201 twice/different UUIDs. PUT same UUID:200 twice/updatedAt changes. | Keys for create and update; never identify a webhook by URL/body or assume duplicate definitions are accidental. |
| **R8: verified against real pactling** | With public reads enabled, anonymous/RO POST401; RW Basic and configured bearer reach validation400. GET200 for all. | Existing method-based read auth alone is insufficient for sensitive history. Server token variable is PACTLING_BEARER_TOKEN; client forwarding uses PACT_BROKER_TOKEN. |
| **S1: supported by exact source** | `src/db/mod.rs::begin` uses BEGIN IMMEDIATE, 5s SQLite busy timeout; nested begin returns a non-owning clone, inner commit is a no-op, dropping owner rolls back. File DB uses WAL/four connections; memory uses one. | A single owning unit of work can combine mutation, trigger rows, receipt and audit. Test file-backed concurrency and cancellation, not only memory serialization. |
| **S2: supported by exact source** | `src/http/{contracts,pacts,verification}.rs` already groups evidence and webhook-trigger inserts in a transaction. Environment creation/update and `service::versions::{put_tag,put_branch_version}` do not uniformly own one. | Share orchestration, preserve event rules, add transaction coverage where missing; do not merely log HTTP responses after commit. |
| **S3: supported by exact source** | `http::auth::granted` returns Access, not a principal; Basic RW wins if credentials overlap, bearer has RW rights, no configured auth opens access. | Add principal information without changing existing permission decisions or credential precedence. Shared credentials identify a credential holder class, not a human/agent individually. |
| **S4: supported by exact source** | Webhook documents may include URLs, body, username and redacted credential fields; verification/pact data and errors can contain arbitrary caller strings. | Never copy request/response documents, URLs, headers or errors wholesale into audit/receipts. |

The probe does **not** establish full-document equality: e.g. captured Location headers and
Ruby's PATCH media handling differ today. Those existing differences are not silently fixed
here. Concurrent duplicate keys, fault recovery and audit retention do not exist today and
remain build acceptance tests, not claims from these sequential probes.

## Decisions and rejected alternatives

1. **Separate opt-in facade:** `POST /pactling/v1/mutations/{action}`, JSON only; existing
   routes ignore keys as today and continue their current retry semantics. Share service
   functions, not loopback HTTP calls or a second rule engine. Reject transparent dedup on
   compatibility routes: it would suppress intentional verification runs and publish events
   observed in R1/R2. Reject adding an arbitrary method/path/body proxy.
2. **Natural identities for ensure-tag/ensure-branch; explicit keys for temporal effects.**
   An unkeyed ensure is state idempotence while membership exists, not a permanent historical
   replay guarantee. Removal then ensure intentionally recreates it. Optional keys offer
   historical replay for these too. Reject dedup solely on body hash, version, webhook URL,
   deployment tuple or environment name. Distinct keys mean distinct intentions, even if
   payloads match. Publish uses the existing pact identity internally plus keyed event replay.
3. **Atomically persist a small receipt, not a compatibility response.** A replay returns
   the original receipt without revalidation against current entity state, rewriting the
   entity, or enqueueing triggers. It authenticates again. Reject cached full webhook/pact
   responses (secret retention, changing links and oversized blobs) and re-rendering a current
   resource as if it were the original result. The receipt links to current resources but
   explicitly represents the original committed operation, not their current state.
4. **One transaction and one logical audit event per keyed operation.** A committed replay
   adds neither mutation nor another audit row. Natural unkeyed ensures may record a new
   `unchanged` outcome on each invocation; that is documented attempt history, not a repeated
   domain effect. Existing unkeyed compatibility writes each retain their own committed
   operation/audit entry. Reject a post-response middleware logger or a second audit DB:
   either can claim success despite rollback, or lose history after commit.
5. **Principal from authentication only.** Store Basic configured username plus role;
   bearer becomes fixed `configured-bearer` (never token/hash); open mode is explicit anonymous.
   Never trust caller Actor/User headers or payload identity. Password/token rotation does
   not change the principal namespace; changing a Basic username does. Bearer rotation
   cannot distinguish old/new holders with current auth, an explicit limitation. Reject an
   external identity service or claiming an agent-supplied label is authenticated identity.
   All holders of the same Basic/bearer credential, and all anonymous callers, share a key
   namespace: they can replay/probe each other's known keys. Keys are not an access boundary.
   Recommend unpredictable random UUIDs to avoid accidental collisions/guessing; receipts
   remain minimal and never reveal payloads. This is not multitenant caller isolation.
6. **History requires current RW authorization when auth is configured**, even with public
   reads. RO users receive 403; unauthenticated callers 401. In intentionally open mode,
   history is public and actor anonymous, matching the deployment's trust boundary. Reject
   blanket ordinary GET authorization (would expose history through public-read bypass).
   No new password or separate audit admin account. Maintainer acceptance is required for
   the RW-only choice and open-mode exposure.
7. **Bounded replay retention without unsafe key recycling.** Proposed defaults: receipts 7 days,
   audit events 90 days; persistent compact key tombstones, capped at 1,000,000 entries.
   After receipt expiry, same key returns410, never executes again; mismatched digest 409.
   Reject deleting keys at TTL and silently treating old retries as new operations. Reject
   timestamp-encoded keys as extra client clock/format complexity. Storage and cap behavior
   below make this safety/capacity tradeoff explicit for acceptance.
8. **Audit committed facts, not every failed attempt.** Validation/auth/conflict failures and
   rolled-back operations leave no domain audit row or reserved key; `outcome` is committed
   `changed` or `unchanged`. Audit insertion/receipt/commit failure fails the mutation closed.
   An exception text or unauthenticated claimed identity is never persisted. Reject writing
   a durable failure row inside a transaction that is then rolled back, or falsely promising
   a complete security log. A separately retained rejection/security-attempt log requires a
   separate design if the maintainer needs it.

## Proposed request, result and error contract

Each action has a closed top-level schema `{idempotencyKey?, target, payload}`. Key is an
opaque case-sensitive ASCII `[A-Za-z0-9_-]{16,128}` identifier; recommend random UUIDs.
Generate one random key per **logical run**, persist it across every transport retry and
process restart; a deliberate new run gets a new key even if the build/version is the same.
A deterministic CI scheme must include its run-attempt identity. A transport retry must
never generate a new key. It is not an authentication secret. Required except for the two ensure operations. No raw
URL, method, headers, actor, SQL, or arbitrary route. JSON body limit 2 MiB for the new facade;
publish batches at most 100 contracts. These new limits do not constrain compatibility routes.
Payload validation reuses existing typed parsers. Per-action schemas must document all
supported fields; extra facade/target fields are rejected, not silently ignored.

| Action | Target / payload mapping to existing service | Retry identity |
| --- | --- | --- |
| `publish-contracts` | target `{}`; payload is existing contracts/publish request, including branch/tags/build URL and contracts | Required key; complete batch atomic, all publication/event effects included |
| `put-pact` | consumer, provider, consumerVersion; payload is exact pact JSON | Required key; legacy PUT rules, not contracts/publish conflict rules |
| `publish-verification` | consumer, provider, pactVersionSha, optional consumerVersion; payload is ResultRequest | Required key per verification **run**; resolve/validate consumer publication association inside transaction, same as legacy metadata intent |
| `record-deployment` | pacticipant, version, environmentUuid; payload `{applicationInstance?:string}` | Required key, including instance; no name-to-environment re-resolution on replay |
| `record-release` | pacticipant, version, environmentUuid; payload `{}` | Required key, protects against reactivation by delayed retry |
| `create-environment` | target `{}`; existing EnvironmentRequest payload | Required key; same name with a different key retains existing duplicate-name rejection |
| `update-environment` | environmentUuid; EnvironmentRequest payload | Required key; existing update semantics, no automatic optimistic concurrency promise |
| `ensure-tag` | pacticipant, version, tag; payload `{}` | Natural tuple; optional key |
| `ensure-branch-version` | pacticipant, version, branch; payload `{}` | Natural tuple; optional key; preserve first-main-branch/branch-head rules |
| `create-webhook` | target `{}`; WebhookRequest payload (scope in body) | Required key; server chooses UUID |
| `update-webhook` | webhookUuid; WebhookRequest payload | Required key; require existing UUID; do not silently recreate a deleted webhook |

Strings retain existing domain validation; facade target names/versions are nonempty and
at most1024 characters. An empty/missing deployment instance is normalized exactly as the
legacy handler. Scope variants of existing webhook collections resolve to the same service
command. No branch rename, tag rename or empty-branch creation is implied by “update”.

Example request (synthetic key/correlation ID):

```http
POST /pactling/v1/mutations/record-deployment
Content-Type: application/json
X-Correlation-ID: 11111111-1111-4111-8111-111111111111

{"idempotencyKey":"55555555-5555-4555-8555-555555555555","target":{"pacticipant":"Payments","version":"abc123","environmentUuid":"22222222-2222-4222-8222-222222222222"},"payload":{"applicationInstance":"blue"}}
```

Every first **keyed** success is 201, including outcome `unchanged`; every keyed replay is
200. Unkeyed ensures alone use outcome-dependent201/200. Clients use `replayed`, not status
alone, to identify replay. Both have the same `receipt` object, except outer replayed:

```json
{"schemaVersion":"1.0","replayed":false,"receipt":{"operationId":"33333333-3333-4333-8333-333333333333","action":"record-deployment","outcome":"changed","actor":{"kind":"basic","name":"ci","access":"write"},"correlationId":"11111111-1111-4111-8111-111111111111","committedAt":"2026-11-02T23:00:00Z","entities":[{"kind":"deployment","id":"44444444-4444-4444-8444-444444444444","pacticipant":"Payments","version":"abc123"}],"auditHref":"/pactling/v1/audit-events/33333333-3333-4333-8333-333333333333"}}
```

A natural unkeyed ensure returns 201 if changed, 200 if unchanged, replayed:false (no stored
receipt to replay). `changed` includes enqueued publication/verification events, not only
changes to the principal row. A successful explicit new key that encounters already-current
state can therefore commit `unchanged`. Entity references are typed allowlisted identifiers
and names/version numbers, never webhook destinations, descriptions, contacts or build URLs.
The request limits do not promise every 100-contract batch fits the receipt limit; long
identifiers can produce RESULT_TOO_LARGE with no committed effects. Batch receipts contain
all affected primary entity refs; dependent tag/branch effects are
represented by the action and documented payload semantics, not an unbounded full DB diff.

On **new facade/history ingress only**, correlation is a validated canonical UUID supplied in X-Correlation-ID or server-generated.
It is not identity or authorization. Legacy ingress ignores X-Correlation-ID entirely,
always server-generates its audit correlation, and adds no new response header or validation
error. A malformed legacy correlation header therefore cannot break a CLI request.
Audit/receipt retain the original operation correlation;
replay response headers carry the current request correlation for diagnostics. Never persist
raw keys or arbitrary correlation strings. Receipt body is frozen before commit; response
serialization and audit projection must succeed before committing, so a serialization error
cannot leave an unrecorded successful mutation.

Errors use JSON, no HAL; only on these new routes:

```json
{"schemaVersion":"1.0","error":{"code":"IDEMPOTENCY_CONFLICT","message":"The key was already used for a different mutation","retryable":false},"correlationId":"11111111-1111-4111-8111-111111111111"}
```

| HTTP / code | When / retry policy |
| --- | --- |
| 400 `INVALID_MUTATION` | Bad action-specific shape, malformed correlation/key, missing required key or domain validation. Optional details contain allowlisted field paths/codes only, not rejected values or raw Ruby/SQL text. Key not consumed. |
| 401 `AUTH_REQUIRED` | Missing/invalid credentials with auth enabled; preserve Basic challenge. |
| 403 `FORBIDDEN` | Authenticated RO principal on mutations or audit history. Existing routes still use their existing401 behavior. |
| 404 `NOT_FOUND` | Unknown action/resource on a first execution; audit ID absent or retained history gone. Never recheck entity existence on a valid replay. |
| 409 `IDEMPOTENCY_CONFLICT` | Same principal/key, different action/target/payload fingerprint. Not retryable unchanged. |
| 409 `MUTATION_CONFLICT` | Existing domain conflict (e.g. contracts/publish content conflict or duplicate environment name). No key consumed; callers must resolve intent. |
| 503 `IDEMPOTENCY_VERSION_UNSUPPORTED` | Stored algorithm is unavailable in this binary (e.g. downgrade). retryable:false until a compatible binary is used; never execute or advise a fresh key automatically. |
| 410 `IDEMPOTENCY_EXPIRED` | Matching key tombstone exists but receipt expired; do not automatically retry with a new key. Inspect history/current state first. |
| 413 `REQUEST_TOO_LARGE` | New facade request/contract-count limit exceeded; rejected before work. |
| 413 `RESULT_TOO_LARGE` | The complete proposed receipt exceeds 64 KiB. Detected after staged work but before commit; all effects/audit/key are rolled back. retryable:false. Caller must deliberately revise/split its operation if appropriate; never auto-retry or truncate entity refs. |
| 415 `UNSUPPORTED_MEDIA_TYPE` | New POST route requires application/json. |
| 503 `MUTATION_BUSY` | SQLite writer/acquire timeout; Retry-After:1, retryable:true, same key. |
| 503 `IDEMPOTENCY_CAPACITY` | Tombstone capacity reached for a **new** key; no mutation. Existing-key replay/conflict still works. retryable:false pending operator capacity change. |
| 500 `INTERNAL_ERROR` | Other storage/audit/commit failure. No raw diagnostics. retryable:true with same key; commit acknowledgement can be uncertain; retrying the same key under the same principal replays any committed receipt. |

In `src/app.rs`, construct a compatibility router with its existing `auth::require` layer,
and a separate router for exactly the new mutation/audit namespaces with a versioned auth
layer using the same credential matcher plus principal result. Merge them **after** attaching
those distinct layers, then apply shared response headers/state. Do not put the existing
`require` around the merged router; do not exempt all `/pactling/` paths. Existing explanation
and UI stay in the compatibility router. The new layer implements JSON 401/403 and history's
RW requirement without public-read bypass. Its fallback/method routes get the same protection.
A malformed correlation header gets 400 with a newly generated safe response correlation ID.

All unknown enum codes must be tolerated; additive codes are minor schema changes. New
required fields/changed meanings require a major namespace. Unsupported methods 405/Allow;
HEAD/OPTIONS must not mutate. Existing normal responses, parser/auth errors and discovery
documents remain byte-compatible; mandatory audit adds the documented storage-failure case.

## Transaction and persistence design

Proposed `service::mutations::execute(db, principal, correlation, command)` owns the unit of
work. Authentication and syntactic limits run first. For keyed requests, use `Db::begin_read()`
to look up a completed key and apply conflict/expiry/replay rules without a writer lock.
Finish/drop that snapshot before acquiring any writer handle (essential with one in-memory
connection). If absent, take `Db::begin()` and **recheck** the key under its writer lock
before any mutable precondition or capacity check. A concurrent winner then becomes a
replay/conflict rather than a second mutation. Natural unkeyed ensures go directly to the
write transaction. Read snapshots never refresh expiry or tombstones; expiry is evaluated
at lookup time. Existing receipts are immutable, so a committed snapshot is sufficient.
Unique index is
`(principal_namespace, key_sha256)` across all actions, not per route. Another action with
that key conflicts. Namespace includes auth kind/configured username, never password/token.
Different principals are independent intentions; receipts cannot be fetched using another
principal's key. The no-re-execution guarantee is **within that principal namespace**.
   Changing Basic username or switching anonymous/open mode to authenticated mode (or back)
   creates a different namespace: the same key can execute as a new intention. Operators must
   reconcile pending operations before auth migration; never automatically retry them under
   a different identity. Password/token rotation within the same configured slot keeps scope.
   Shared-credential callers are the same principal, as Decision 5 documents.
History authorization is separately global RW, not tenant isolation.

Fingerprint version 1 hashes a domain-separated tuple of action + target + complete payload,
using SHA-256 over recursively key-sorted compact JSON, preserving arrays and **parsed serde_json::Value numbers**, not raw numeric lexemes. Whitespace/object ordering do not conflict; array reorder, ignored payload
fields, changed metadata, base64 strings or credentials inside webhook configuration do.
This deliberately prefers false conflicts over silently accepting a changed intention.
Authorization, correlation and idempotencyKey are excluded. Do **not** reuse the pact SHA:
`service/content.rs::pact_version_sha` intentionally ignores/reorders some content. Version
fingerprint algorithms explicitly; never reinterpret existing rows after an upgrade.

Mechanism: a facade-only recursive serde Deserialize visitor constructs Value, rejecting
any decoded duplicate object key at any depth before insertion (including escaped spellings
of the same key). Use serde_json's existing numeric visits/serialization, no arbitrary_precision
feature and no changes to legacy parsing. Probe with exact serde_json1.0.154: `1` serializes1;
`1.0`, `1e0` and `1.00` serialize1.0; ordinary Value parsing silently discards the first duplicate
key. Thus1 and1.0 conflict; the other three forms match. Any numeric rounding is the existing
Value/domain parse semantics, not a promise to preserve mathematical arbitrary precision.
A new duplicate-checking visitor and frozen digest vectors must establish this during build.

Look up stored fingerprintVersion before computing a comparison digest. Retain old algorithm
implementations and golden vectors across upgrades, applying the stored version to the
**incoming** payload (old payload storage is unnecessary). If a running binary cannot evaluate
the stored version, return `IDEMPOTENCY_VERSION_UNSUPPORTED`, never execute or report normal
payload conflict. Library upgrades must pass all historic vectors; otherwise preserve the
old parser/algorithm or explicitly reject that version, never reinterpret existing hashes.

The same existing-key decision runs in the read snapshot and, on a miss, again in the
write transaction. Only the latter may execute new work:

1. Existing key: compare fingerprint before domain lookups; enforce persisted expiry at lookup even if maintenance has not run. Different =>409; expired receipt
   =>410; otherwise copy frozen receipt and finish the transaction without writes. No audit
   replay event, clock refresh, trigger or entity lookup. Deletion after commit does not make
   a replay recreate anything. Current auth is still required.
2. New key or natural ensure: validate domain state, run the existing mutation and transactional
   webhook scheduling, produce a typed effect summary. Inner functions use only the transaction
   handle and do not own the commit. Natural identities continue using existing unique indexes.
3. Insert audit event and entity references, construct/serialize receipt (max 64 KiB for the new
   facade), insert key fingerprint/receipt if keyed, then commit once. Audit/receipt failure
   rolls back the entire batch, trigger queue and implicit pacticipant/version/tag/branch
   writes. New facade oversized receipts return 413 RESULT_TOO_LARGE before commit. Legacy audit projections
   do not acquire new length limits that would reject otherwise-valid Ruby-compatible inputs.
4. Return only after commit. Lost response/restart retries use the durable receipt. Aborted
   work/cancellation before commit leaves no key reservation; no orphan “in-progress” state.
   A timeout while commit completes is ambiguous to the client but not to same-key lookup.

No generic “success means response was sent” claim. `committedAt` is the UTC timestamp
chosen immediately before the successful transaction commit, not an exact fsync completion
measurement. Webhook dispatch remains the existing at-least-once network process: one keyed
operation enqueues one intended set of trigger rows, but cannot guarantee exactly-once
external delivery. Network calls must not run while the mutation holds the writer lock.

Proposed migration `0006_mutation_audit.sql` (renumber if main advances):

- `audit_events`: `sequence INTEGER PRIMARY KEY AUTOINCREMENT` (never reused after pruning),
  operation UUID unique, actor kind/name/access, action,
  changed/unchanged outcome, original correlation UUID, UTC time, ingress (`compatibility`
  or `agent_v1`), coverage/version metadata. No key/fingerprint/request/response blobs.
- `audit_entities`: event FK plus typed identity snapshot (name/version/UUID/content SHA as
  applicable), immutable event ordering. Do not cascade history deletion from domain rows;
  version labels/identities must remain attributable after deletion or ID reuse.
- `mutation_key_usage`: singleton counter for committed key rows/tombstones, initialized
  from a one-time migration count (zero for a new database). Under the writer transaction,
  conditionally increment only when below configured cap, and insert the new key in that
  same transaction; rollback undoes both. No per-write COUNT scan. Receipt pruning does
  not decrement it because tombstones remain.
- `mutation_keys`: principal namespace + SHA-256 key unique, fingerprint/version, operation
  UUID, immutable small receipt, created/receipt-expiry timestamps. Expired rows retain only
  key identity/fingerprint/operation/time as tombstones. No cascade when audit expires.

Audit is an allowlist projection from typed results. Never store auth headers, passwords,
webhook headers/body/URL/username, pact contents, verification diagnostics/testResults,
contacts, build URLs, arbitrary exception text, client IP/user-agent or raw request bodies.
Request fingerprints belong only in the private idempotency table, never audit/history or
logs; they are not a secret-storage/encryption mechanism. Existing domain storage is unchanged.
Names/version identifiers and configured Basic usernames are intentionally auditable metadata:
operators must not place credentials in those identifiers. No sanitizer can prove arbitrary
caller text contains no embedded secret. Test explicit secret canaries in every designated
secret-bearing field against audit rows, receipts, errors and history, not just rendered HTML.

## Authorized history and retention

`GET /pactling/v1/audit-events` and `GET /pactling/v1/audit-events/{operationId}` only. List
filters: action, actor kind/name, correlationId, entity kind/name/version and UTC from/to;
allowlisted exact-match parameters, SQL bound values. Default pageSize 25, max 100. Return
`schemaVersion`, `coverage:"listed_committed_mutations_only"`, `retentionDays`, `items`,
`nextCursor`, and `highWaterId`. Event item is the safe receipt metadata without replayed;
no idempotency key/digest or stored secret-bearing documents. Existing open enum policy applies.

Keyset pagination uses increasing sequence IDs; first call fixes highWaterId. Cursor encodes
last ID, high-water and normalized filters, validates types/bounds and requires identical
filters on continuation. It is a locator, **not** a bearer capability: authenticate every
page/detail request. Later commits are excluded; retention can remove rows between pages,
so this is not a long-lived atomic snapshot. Use `Db::begin_read` for each page/detail read;
no writer lock, no “last viewed” update, no cleanup on read. Invalid cursor 400, missing detail 404.
Return relative links under the configured path prefix, built from typed identifiers, not
stored caller URLs. Entity links may become404 after domain deletion; history remains valid.

Proposed server settings (exact CLI/env spelling settled during build, documented together):
receipt retention 7 days, audit retention 90 days, ledger maximum 1,000,000 entries; `0` audit
retention means keep forever, never “disable auditing”. Existing keys/tombstones are not
silently discarded by changing configuration. Record receipt expiry at original commit;
configuration changes affect new receipts only. The history response reports effective audit
retention and its coverage. UTC clock jumps can alter expiration timing, but cannot make an
expired/tombstoned key execute again.

An hourly server maintenance job converts expired receipts to tombstones and removes audit
rows older than retention in bounded 1,000-row transactions, off request/read paths. It is
not itself an audited domain mutation. No network export or automatic VACUUM; document WAL,
freelist reuse, backup consistency and operator-scheduled compaction. Keep evidence identifiers
in receipts even if an operator configures audit retention shorter than receipt retention;
the audit link may then 404, without breaking replay. Crash/restart resumes maintenance safely.

Storage is O(retained audit events/entity refs + retained receipts + **all accepted keys**).
At rate r keyed operations/day, tombstones reach the default cap after about 1,000,000/r days;
audit retains about 90r operations plus unkeyed traffic. Measure actual bytes/row, indexes and
WAL on realistic fixtures before claiming sizes. Any trusted writer can consume the shared ledger cap, including callers in open mode;
this is a capacity boundary, not denial-of-service isolation. Per-principal limits would not
isolate the common single-Basic/bearer/anonymous deployment; finite tombstone TTL would
reintroduce unsafe late retries. Do not implement either as an assumed fix. Operators must
restrict write access and monitor growth/capacity; expected rates and cap approval are open.
At capacity, reject new keyed mutations
before side effects, **including keyed ensures**; allow replay, **unkeyed** natural ensures
and legacy requests subject to ordinary
storage availability. Increasing the cap requires operator capacity planning. No supported
online “forget this key” endpoint: removing tombstones would void delayed-retry safety.
Time retention does **not** hard-bound audit rows/bytes during a burst: unkeyed unchanged
writes add history too. No audit row cap silently evicts recent history or skips audit
outcomes. Measure this overhead in Task 3 before expanding coverage; an optional count/byte
retention policy needs maintainer acceptance and visible truncation metadata. Storage-full
remains fail-closed, including legacy writes. A restore to an older backup can lose acknowledged operations/keys; document that recovery
boundary and reconcile externally before retrying. This is not a distributed exactly-once system.

## Future MCP writes

A future separately approved MCP writer would use this facade with the existing configured
broker auth, preserve a caller's key across timeouts, expose conflict/expiry/unknown-commit
states, and return the safe receipt. It must not generate a fresh key on each transport retry,
reuse the GET-only client to smuggle writes, or bypass the shared service/audit transaction.
This plan does not add tools, choose their schemas/approvals, or change the current five
read-only tools, annotations, limits, or database-nonmutation guarantee.

## Ordered implementation tasks and checks

Each task is a separate test-first change: assertion-red before code; characterize existing
behavior before refactoring; mutation-check every new/changed guard and immediately restore.
Do not change protected Ruby fixtures to make a regression pass. Record design amendments
before departing from an accepted plan. Names below are proposed concrete tests/files.

1. **Capture/characterize compatibility and side effects.** Extend the matching phase sections
   of `scripts/capture-ruby-fixtures.sh`, add isolated retry captures under
   `tests/fixtures/ruby-broker/2.121.2/`, and real CLI captures under `tests/fixtures/cli/`.
   Existing bytes untouched; zero differences on recapture. Add `tests/mutation_compatibility.rs`:
   `legacy_retry_statuses_and_resource_identity`, `unchanged_publish_still_schedules_event`,
   `delayed_deployment_retry_restores_old_version`, `release_retry_can_reactivate_support`,
   `branch_retry_keeps_newer_head`, `existing_idempotency_header_does_not_change_legacy_routes`.
   These are characterization tests expected green today; missing atomic audit tests below
   supply assertion-red. Include both pact publish APIs and scoped/UUID webhook variants.
2. **Authenticated principal and persistence seam.** Files `src/http/auth.rs`,
   `src/db/{mod,audit,idempotency}.rs`, migration above, `tests/{auth,audit_storage}.rs`.
   Keep existing auth predicate/precedence byte-compatible; add request principal extraction
   and the explicit split-router layering above in `src/app.rs`.
   Tests `principal_matches_existing_auth_decision`,
   `legacy_ro_post_stays_empty_401_facade_ro_post_is_json_403`,
   `legacy_malformed_correlation_header_is_ignored`, `actor_is_not_taken_from_headers`,
   `audit_and_receipt_rollback_with_owner`, `nested_mutations_cannot_commit_owner`,
   `audit_survives_domain_deletion`, `key_namespace_excludes_credentials`,
   `migration_preserves_existing_broker_data`. Reject any accidental use of pool Db inside tx.
3. **Shared audited unit of work for the full scoped legacy write surface.** Files
   `src/service/{mutations,publish,verification,environments,versions,webhooks}.rs`,
   `src/http/{contracts,pacts,verification,environments,versions,webhooks}.rs` and audit DB.
   Extract result/effect summaries; preserve parser/rule differences between native routes.
   Tests `every_scoped_write_has_committed_audit`, `implicit_tag_branch_and_trigger_are_atomic`,
   `batch_failure_leaves_no_entities_or_audit`, `audit_insert_failure_rolls_back_mutation`,
   `legacy_documents_and_statuses_unchanged`,
   `legacy_audit_failure_is_atomic_storage_error`, `read_only_requests_leave_audit_unchanged`.
   Use database fault injection to fail after domain writes and before receipt/audit/commit;
   cover environment uniqueness races and tag/branch creation on a fresh participant.
   Measure write latency and audit bytes per operation now (changed and unchanged writes),
   before extending the facade; record projection/index/WAL growth and realistic peak rate.
4. **Keyed facade, receipt and error schemas.** Files `src/http/mutations.rs`, HTTP router,
   `src/service/{mutations,idempotency}.rs`, `src/db/idempotency.rs`,
   `docs/api/mutations*.schema.json`, `tests/{idempotent_mutations,mutation_errors}.rs`.
   Table-driven all actions: `same_key_replays_original_receipt`, `same_key_changed_intent_409`,
   `cross_action_key_reuse_conflicts`,
   `shared_credential_callers_share_key_namespace`, `different_actor_keys_are_isolated`,
   `failed_validation_does_not_consume_key`, `natural_membership_ensure_is_idempotent`,
   `first_keyed_unchanged_is_201_and_replay_is_200`,
   `replay_after_intervening_update_or_delete_does_not_restore_state`,
   `verification_new_key_is_new_run`, `nested_and_escaped_duplicate_json_keys_rejected`,
   `fingerprint_numeric_value_semantics_are_frozen`,
   `old_fingerprint_version_still_matches`, `unsupported_fingerprint_version_never_executes`,
   `fingerprint_covers_secret_and_metadata_changes`, `errors_never_echo_input`,
   `serialization_failure_rolls_back_before_commit`,
   `max_identifier_batch_receipt_overflow_rolls_back_all_effects`,
   `auth_namespace_migration_is_a_new_intention`,
   `facade_put_pact_rejects_duplicates_legacy_keeps_last_value`. File DB tests:
   `concurrent_same_key_commits_once`, `replay_read_does_not_block_writers`,
   `absent_read_then_concurrent_insert_rechecks_under_write_lock`, `concurrent_conflicting_key_has_one_winner`,
   `lost_response_and_process_restart_replay`, `cancelled_transaction_leaves_no_reservation`,
   `busy_returns_retryable_error`. Assert domain/audit/key/trigger counts, not only HTTP 200.
5. **History, authorization and lifecycle.** Files `src/http/audit.rs`, audit DB,
   `src/service/audit_retention.rs`, `src/{config,app}.rs`, `docs/api/audit*.schema.json`,
   `tests/{audit_history,audit_retention}.rs`. Tests
   `history_ignores_public_read_bypass`, `history_denies_ro_and_cross_auth_replay`,
   `open_mode_history_is_explicitly_anonymous`, `history_pages_respect_filters_and_high_water`,
   `history_is_read_only_snapshot`,
   `audit_sequence_not_reused_after_pruning_all_rows`,
   `replay_reauthenticates_after_rw_revoke_or_downgrade`, `audit_canaries_never_appear`,
   `expired_receipt_never_executes_again`, `expired_key_changed_payload_conflicts`,
   `capacity_rejects_new_key_but_allows_replay`,
   `capacity_rejects_keyed_ensure_but_allows_unkeyed_ensure`,
   `capacity_counter_rolls_back_and_cannot_oversubscribe`,
   `capacity_lookup_uses_single_metadata_row_at_million_keys`, `pruning_never_deletes_key_tombstones`,
   `pruning_restart_and_clock_changes_are_safe`. Seed approximately1M key rows; inspect
   query plans to rule out a ledger-wide count in the mutation path and record writer-lock
   duration, avoiding machine-specific timing assertions in unit tests. Revocation tests restart with auth still enabled: old Basic password/token 401,
   the former RW credential now RO 403, no receipt data returned. Turning all auth off is
   explicitly switching to anonymous open mode, not a revocation/security guarantee.
   Include concurrency with publish/deployment
   while a history read snapshot is open, and the single-connection in-memory caveat.
6. **Real-client verification, documentation and independent review.** Add
   `e2e/phase6_mutations_audit.sh` (name/order must not collide with main), using built binary,
   real Pact CLI and receiver: native CLI writes retain exact results; keyed HTTP client loses
   a response/retries; one verification and one intended trigger set remain; late deployment
   retry does not roll back newer state; restart and authorization/canary probes. MCP real
   client still calls five reads without audit/DB effects. Document route/limits/error codes,
   natural-vs-keyed guarantees, identity limitations, secrets, retention/tombstones/restore,
   and examples in `docs/api/idempotent-mutations-audit.md` and README deliberate differences.
   Explicitly list facade duplicate-key rejection/size limits versus legacy parsing, the
   mandatory audit storage-failure case, and the auth-namespace migration safety boundary.
   Run fmt, clippy/all tests with default and `--features mcp`, nine/current release contract
   tests, full `bash e2e/run.sh`, all 108 captured matrix cases, matrix/deployment full documents,
   before/after Accept variants and real CLI request/response recapture. Measure transaction
   latency/storage growth with audit enabled and during pruning; no unsupported performance
   claims. Complete non-local `agent-review code`, probe/fix verified findings test-first and
   re-review; publish the per-guard mutation table. Never merge without owner approval.

During this plan task: only the existing binary was built and probed; no production test
suite is claimed as newly run. No buildx builder was created. Cleanup uses this clone's
`cargo clean` and only `pactling-audit-probe` compose `down -v`.

## Questions requiring maintainer acceptance

1. Accept the separate `/pactling/v1/mutations/{action}` facade and small receipts, leaving
   legacy retries unchanged (including duplicate events), with audit coverage on the listed
   legacy writes and fail-closed audit storage errors? This is recommended over changing
   compatibility routes via a header.
2. Accept committed-fact-only history, with failed/unauthorized/rolled-back attempts absent,
   and the explicit excluded-write coverage? A durable failure/security log is separate work.
3. Accept RW-only history when auth is configured, public anonymous history in open mode,
   and shared Basic/bearer credential attribution (including shared key namespace) rather than
   invented per-agent identity?
4. Accept 7-day receipts, 90-day audit, permanent key tombstones and a 1,000,000-key fail-closed
   cap (operator-expandable), plus 2 MiB/100-contract/64 KiB new facade limits? This avoids unsafe
   old-key reuse **within one principal namespace** but commits to storage growth. Receipt
   limits can reject large valid input batches with full rollback; auth-namespace changes
   require reconciliation rather than automatic retry. If permanent tombstones are unacceptable,
   reconsider timestamp-bound keys before implementation, not silent expiry/re-execution. What operation rate/peak audit growth must the deployment
   support, and is time-only audit retention sufficient or is a visibly truncating count/byte
   policy needed? No new per-caller isolation is assumed for shared credentials.
