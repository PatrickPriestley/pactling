---
change_id: deployment-explanations
status: draft
accepted_by: null
accepted_on: null
divergences: []
---

# Plan: evidence-backed deployment explanations (#10)

This is a plan only, for maintainer review before any build. It proposes item 6 of
the requested roadmap work, scoped to [#10](https://github.com/PatrickPriestley/pactling/issues/10).
[#5](https://github.com/PatrickPriestley/pactling/issues/5),
[#11](https://github.com/PatrickPriestley/pactling/issues/11), and
[#17](https://github.com/PatrickPriestley/pactling/issues/17) are context, not approved
implementation scope. No production code, migrations, or compatibility fixtures change
in this PR. There is no embedded LLM, mutation execution, system-context index, UI,
MCP interface, or claim about live traffic, health, or unrecorded dependencies.

## Problem

The existing broker can decide whether recorded contracts permit deployment, but its
compatibility response primarily communicates the rationale through Ruby-compatible
prose. A client needs to distinguish allowed, blocked, and unknown decisions without
parsing that prose, identify the exact contract and application versions responsible,
and determine the next concrete work to do. Those explanations must retain the same
verdict, including counterintuitive Ruby precedence, expose the limits and timestamps of the
recorded evidence, and remain useful to humans and automation without becoming a second
authoritative decision engine.

## Evidence and current behavior

Evidence labels: **source-supported** means inspected at main `74ebe81`; **fixture-backed**
means read from committed captures of Ruby 2.121.2, not newly executed against Ruby in
this planning task. Everything described as proposed below is an **unimplemented design**,
not a verified endpoint or a runtime snapshot guarantee. The four issues were read with
`gh issue view` against GitHub. Their historical triage results are not current test results.

Source-supported findings:

- `src/service/matrix.rs::MatrixQuery::parse` handles ordered `q[]` selectors (pins,
  latest, branch, mainBranch, tag), environment, latestby, counterpart latest/tag,
  flat can-i-deploy parameters, and version-specific ignores. `to` means latest with
  a tag, not an environment. Omitted latestby stays omitted; it is not implicitly cvpv.
- `matrix` resolves every directed pair for multiple selectors and both consumer/provider
  roles for a single selector. It validates participants/environment, records missing
  selector versions and warnings, deduplicates overlapping rows, applies latestby,
  applies ignores, and calls `summarize`. Environment resolution uses currently deployed
  and currently supported released versions (`src/db/environments.rs::version_ids_in_environment`).
- `apply_latestby` groups cvpv/cvp/cp after selection. It preserves the first null provider
  row; otherwise it takes the last maximum provider creation order after Ruby query
  ordering. Explanations must describe this selection, not substitute semver or the
  newest verification globally. Some candidate versions with no verification do not
  become distinct matrix rows; an explanation must not invent additional blockers.
- `summarize` is already synchronous and database-free, but private and dependent on
  `Notice.kind == "error"` to detect missing selector versions. It counts ignored rows
  separately, then successes, failures, and unverified rows. Precedence is: missing
  selector version => false; otherwise any unknown row => null; otherwise any failed
  row => false; otherwise true. Thus failed + unknown => unknown. Zero considered
  rows can mean allowed ("There are no missing dependencies"), including all ignored.
- `Row` contains the publication SHA/content, participant/version records, latest
  verification for that content/provider version, and ignored flag. `Matrix` already
  contains tri-state deployable, counts, rows, notices, and has_ignores.
- A blank compatibility provider version does **not** always mean no version resolved:
  environment, latest, and pinned selection can know a candidate version but deliberately
  set `Side.version = None` when no verification exists. The candidate is currently
  retained only in description prose. Preserve it at resolution time; do not parse prose.
- `src/http/matrix.rs` calls the same `service::matrix` for `/matrix`, the integration
  route, and `/can-i-deploy`. The last adds required-field validation (participant,
  version, environment or to-tag). Its summary copies the tri-state verdict and counts;
  notices supply the reason text. Rows render version/tag/branch/environment links,
  publication links, and verification links. Rendering performs additional database
  reads using `state.db`; currently those reads and resolution are not enclosed by
  these handlers in one snapshot.
- `Db::begin` in `src/db/mod.rs` provides a shared `BEGIN IMMEDIATE` transaction;
  dropping rolls back and only its owner commits. This is a write-locking transaction,
  not a read-only snapshot API. All methods must use the transactional Db, never the
  pool Db, while it is open (especially the single-connection in-memory tests).

Fixture-backed baseline: `tests/fixtures/ruby-broker/2.121.2/phase4/matrix-regressions.json`
contains **108** cases: **39 true, 51 false, 18 null**. Its harness is
`tests/matrix_regressions.rs::captured_matrix_regressions_match_ruby`. Relevant cases
include `unknown_and_failed_is_null`, the missing-pin cases, `unverified_pin_ignored`, and the latestby cases. Full HAL documents also
have the `tests/matrix.rs::matrix_documents_match_the_ruby_broker` oracle.

An explanation needs additional typed resolution provenance, reason codes, evidence
identities, action targets, and evaluation/snapshot metadata. Existing prose and `Matrix`
alone cannot distinguish every unresolved selector, known-but-unverified candidate,
ignored request warning, or deployed-versus-released source accurately.

## Shared decision and dependency on #5

A **narrow prerequisite from #5 is required before shipping #10**: one owned immutable
`DecisionEvaluation` containing the compatibility `Matrix`, typed resolution outcomes,
row dispositions and typed decision causes, obtained from a consistent database read.
The compatibility renderer and explanation renderer consume that same object. Neither
renderer queries evidence or calls the evaluator. This does not require #5's entire
batching/performance redesign or invariant expansion, and does not authorize doing #5
in this plan PR. The maintainer has selected a separate prerequisite PR first (pure evaluator plus
read-only snapshot), then the #10 PR. Rebase and adapt to its accepted types before
building #10; neither implementation is authorized by this plan-only revision.

Proposed prerequisite seam (names subject to that review):

1. `resolve_matrix(db, query)` retains exact resolved versions, candidates represented
   by blank rows, missing selectors, selection source and exclusion/ignore provenance.
2. `evaluate(resolved)` is pure and runs **once**. It owns existing latestby, ignores,
   summary counts, verdict precedence and typed causes. Preserve the compatibility
   notice text/order exactly through a projection from the same facts. No message parsing.
3. `DecisionEvaluation` owns the result and all data needed to render both views,
   including the full version records, branch/tag decorations, computed tag-latest flags,
   environment entities and membership source records used by either view, not lazy IDs
   requiring later queries. The legacy `matrix` wrapper may return its
   compatibility projection for callers; the explanation handler must not call it and
   then evaluate again. No re-resolution to discover action targets.
4. Both HTTP surfaces call the same orchestration function. A test obtains one evaluation,
   renders both views, and asserts the evaluator was called once and the renderers need
   no Db. Separate HTTP requests can legitimately differ if data changed between them.

### Read-only snapshot API for the prerequisite

Add `Db::begin_read(&self) -> Result<ReadSnapshot>` next to `Db::begin/commit` in
`src/db/mod.rs`. It acquires a connection and opens a deferred transaction with plain
`BEGIN`; the first statement inside it is a read of broker data, establishing the
snapshot. It must never call `Db::begin()` or upgrade to `BEGIN IMMEDIATE`. Under WAL,
the intended contract is one consistent view while other connections can commit writes.
This is a required design/probe outcome, not a runtime claim established by this plan.

`ReadSnapshot` is a separate read-only handle exposing only the read operations needed
for resolution and decorations (a shared read-query interface may avoid duplication).
It must never write: no mutation methods, transaction upgrade, raw writable executor,
`Deref<Target=Db>` or pool escape hatch. An owning `ReadSnapshot::finish(self)` ends the
read transaction; dropping on error/cancellation rolls it back and releases the connection.
Borrowed read access cannot finish the owner's transaction. `begin_read` on an existing
write-transaction Db must fail explicitly rather than silently reuse a write lock or
acquire a second connection: explicitly reject `Conn::Tx` rather than copying the
clone-on-nesting branch of `Db::begin`. Reads within business writes keep their existing
contract.
Document these APIs beside the existing write transaction APIs, which remain unchanged.

All resolution, verification, environment membership and version decorations (including
every lookup currently in `version_document`) use this handle before it finishes.
Materialize the evidence, close the snapshot promptly, then run the pure evaluator and
render from owned data. No pooled rereads, external calls, or serialization inside the
snapshot. No snapshot ID promises historical replay or reserves permission to deploy.

Required probe in the prerequisite PR: use the actual pinned sqlx/SQLite stack and a
fresh **file-backed WAL database with separate reader/writer connections**. Establish
an explanation snapshot with a real data read, pause it with a test barrier, then publish
or record a deployment through the real service/API on another connection. Assert that
the write completes and commits **before** releasing the reader barrier, with a bounded
timeout to detect blocking. Resume multiple evidence/decorations reads and assert every
field is from the pre-write snapshot; a new snapshot must see all committed changes.
Also run the inverse schedule (writer commits before the snapshot's first read) and
assert an entirely post-write result. Use multi-row business operations so mixed states
are detectable. Cover both publication and deployment, lock/connection release on
errors/cancellation, and continued successful writes after snapshot cleanup. Record the
real engine versions and results; a mock or sequential test cannot establish this claim.

The in-memory database currently has one pool connection: a writer may wait for that
connection, which is not SQLite write-lock contention. Test one-connection resolution
and cleanup without nested pool access, but use the file/WAL probe for writer concurrency.
Long readers can delay WAL checkpoint progress; keep snapshot lifetime bounded by
loading the needed data and measure a larger matrix as well. Never substitute
BEGIN IMMEDIATE or pooled independent reads if the probe fails.

## Proposed API and request contract

One read-only extension route in v1:

`GET /pactling/v1/can-i-deploy/explanation?<existing can-i-deploy query>`

It returns `application/json`, status 200 for all three valid decisions, and
`Cache-Control: no-store`. `GET, OPTIONS` are supported; other methods receive 405 with
Allow. Use existing authentication/read-access middleware, including configured public
reads and read-only credentials. No auth bypass or new root HAL relation. Existing
routes, response bodies, content types, links, notices and index documents remain unchanged.
A matrix explanation route is possible later work, not a v1 endpoint or explicit-mode option.
Document the extension in README's deliberate differences and a dedicated API doc.

Use the same `MatrixQuery::parse` and required-field validation as `/can-i-deploy`:
participant and version plus an environment or to-tag. Preserve all query options the
existing route accepts, including latestby and ignores, ordered selectors and duplicates;
do not add defaults or stricter validation. `to` retains its latest-tag meaning.
Unsupported keys retain the existing parser behavior. No extra policy parameters or
JSON request body are introduced. Record the original query in the response. For example:

```text
GET /pactling/v1/can-i-deploy/explanation?pacticipant=Checkout&version=1&environment=production&latestby=cvp
```

Unknown participant/environment and invalid required fields are request errors, not
unknown decisions. Preserve counterpart status and validation semantics with a versioned
extension JSON envelope `{ "schemaVersion": "1.0", "error": { "code": "INVALID_QUERY",
"details": <existing errors value> } }` for 400. Authentication responses remain owned by
the existing middleware; unsupported versions remain 404; internal failures are 500 with
no decision and no internal detail.

`compatibility.deployable` is copied from the decision, never recomputed from reasons:
true maps to `allowed`, false to `blocked`, null to `unknown`. `compatibility` also holds
success/failed/unknown/ignored counts and `hasIgnores`; the extension always includes
ignored=0, while the legacy projection continues omitting it when no ignore was requested.
`summary` is deterministic human text derived from typed causes, not input to the verdict.

## Response structure, evidence and next actions

All fields in the examples below are required, with explicit nulls and empty arrays.
The three examples are complete proposed documents for separate illustrative database
states, not real broker captures. Names, timestamps, SHA and record UUIDs are illustrative.

- `evaluation.id` is a request-local UUID, not a database revision or retrievable resource.
  `startedAt` precedes snapshot acquisition; `evaluatedAt` is read once after all snapshot
  evidence is materialized. `snapshot.kind = read_transaction` describes isolation, not durability.
  The broker version and engine revision (build commit when available, null otherwise)
  identify the evaluator. The query is part of the context.
- `resolution.selectors` records each input selector index, its typed `requested` selector, and its exact versions
  or an empty list plus a typed missing reason. The requested selector has participant,
  version/tag/branch (nullable), latest and mainBranch booleans. `target` records environment UUID/name or
  tag/latest/any selection. Its tagged variants are `environment` (name, uuid, href),
  `latest` (tag or null), `selected_versions` (selector indices), and `any`. Environment
  context remains in the request even when multiple selectors take precedence over it.
  `ignores` records each index, participant, requested version or null and resolution
  status (`resolved`, `participant_missing`, `version_missing`), so ignore references
  do not require reparsing the raw query. `candidates` records exact counterpart versions and source
  references, including candidates not represented as verified rows. `excluded` explains
  candidates/rows removed by latestby or the existing resolver, with references and code;
  it does not add them back to counts. Grouped rows retain provenance from all contributing
  selector indices. Do not sort selectors to achieve deterministic serialization.
- `rows` are in the compatibility row order, with response-local IDs (`row-1`, etc.).
  `consumerVersion` and `providerVersion` match compatibility nullability exactly;
  `candidateRefs` point at separately preserved exact candidate identities. `disposition`
  is considered or ignored. `contract` includes algorithm `sha1`, existing pact SHA,
  immutable content-addressed href and a publication href. Do not change pact hashing.
  `verification` is null or contains number, success, verifiedAt and an existing result
  href scoped by provider, consumer and contract SHA. Use existing link encoding/base-URL
  helpers; links are not guaranteed to survive deletion. No raw contract or credentials
  are copied into the explanation.
- References in reasons/actions are JSON Pointers into this document (`evidenceRefs`),
  allowing precise evidence even for null/missing facts. Public hrefs are additional
  navigation, not a requirement that an external service be contacted. Environment
  membership source records include kind deployed/released, UUID, href, application
  instance (null for release), and recordedAt; multiple records may select the same
  version. Missing selectors reference the selector and its empty resolution, never an
  invented version URL. A positive empty matrix means no recorded required rows under
  this query, not proof of complete system coverage.
- `reasons` entries have code, role (`decisive`, `supporting`, `advisory`), message,
  evidenceRefs and actionIds. Role is assigned from the typed decision causes: missing
  selector reasons dominate, else unverified reasons, else failures, else success/empty.
  Non-dominant row facts remain supporting; ignored/excluded facts are advisory. All messages are deterministic templates, with no prose parsing.
- `nextActions` are suggestions only: id, type, message, evidenceRefs and typed `target`.
  Deduplicate by action type and exact target; order by first reason occurrence. A verify
  action names consumer/provider versions and contract SHA, a fix action names the failed
  result, and missing selection suggests recording/publishing or correcting that exact
  request. Only candidates contributing to a considered unknown row can be required verification
  targets; excluded and ignored row contributions never generate remediation work.
  Generate verify actions from considered unknown rows and their contributing candidate
  references, never by scanning all `resolution.candidates`. A version occurring in both
  a retained row and an excluded row is not globally banned: its action must be justified
  by the retained unknown row and exact contract, not the excluded contribution. If that row
  retains multiple contributing candidates, emit one action per such exact candidate;
  for no resolved version suggest `RESOLVE_TARGET_VERSION` first, never a speculative verify.
  No automatic publish, deployment recording, ignore, retry loop or shell command.

### Complete allowed example

```json
{
  "schemaVersion": "1.0",
  "decision": "allowed",
  "summary": "All considered contract verifications succeeded.",
  "compatibility": {"deployable": true, "success": 1, "failed": 0, "unknown": 0, "ignored": 0, "hasIgnores": false},
  "request": {"query": "pacticipant=Checkout&version=1&environment=production&latestby=cvp"},
  "evaluation": {"id": "00000000-0000-4000-8000-000000000001", "startedAt": "2026-11-01T12:00:00Z", "evaluatedAt": "2026-11-01T12:00:01Z", "brokerVersion": "0.1.0", "engineRevision": null, "snapshot": {"kind": "read_transaction", "retained": false}},
  "evidence": {"completeness": "complete", "coverage": "recorded_only", "currentness": "not_checked"},
  "resolution": {"selectors": [{"index": 0, "pacticipant": "Checkout", "versions": ["1"], "missingReason": null, "requested": {"pacticipant": "Checkout", "version": "1", "latest": false, "tag": null, "branch": null, "mainBranch": false}}], "target": {"kind": "environment", "name": "production", "uuid": "00000000-0000-4000-8000-000000000010", "href": "/environments/00000000-0000-4000-8000-000000000010"}, "candidates": [{"id": "candidate-1", "pacticipant": "Payments", "version": "2", "selectorIndices": [0], "sources": [{"kind": "deployed", "uuid": "00000000-0000-4000-8000-000000000020", "href": "/deployed-versions/00000000-0000-4000-8000-000000000020", "applicationInstance": null, "recordedAt": "2026-11-01T09:00:00Z"}]}], "excluded": [], "ignores": []},
  "rows": [{"id": "row-1", "consumer": "Checkout", "consumerVersion": "1", "provider": "Payments", "providerVersion": "2", "candidateRefs": ["/resolution/candidates/0"], "disposition": "considered", "contract": {"hash": {"algorithm": "sha1", "value": "1234567890abcdef1234567890abcdef12345678"}, "href": "/pacts/provider/Payments/consumer/Checkout/pact-version/1234567890abcdef1234567890abcdef12345678", "publicationHref": "/pacts/provider/Payments/consumer/Checkout/version/1"}, "verification": {"number": 1, "success": true, "verifiedAt": "2026-11-01T10:00:00Z", "href": "/pacts/provider/Payments/consumer/Checkout/pact-version/1234567890abcdef1234567890abcdef12345678/verification-results/1"}}],
  "reasons": [{"code": "ALL_REQUIRED_VERIFICATIONS_SUCCESSFUL", "role": "decisive", "message": "All considered contract verifications succeeded.", "evidenceRefs": ["/rows/0/contract", "/resolution/candidates/0", "/rows/0/verification"], "actionIds": []}],
  "nextActions": []
}
```

### Complete blocked example

```json
{
  "schemaVersion": "1.0",
  "decision": "blocked",
  "summary": "Payments version 2 failed verification of the Checkout version 1 contract.",
  "compatibility": {"deployable": false, "success": 0, "failed": 1, "unknown": 0, "ignored": 0, "hasIgnores": false},
  "request": {"query": "pacticipant=Checkout&version=1&environment=production&latestby=cvp"},
  "evaluation": {"id": "00000000-0000-4000-8000-000000000002", "startedAt": "2026-11-01T12:00:00Z", "evaluatedAt": "2026-11-01T12:00:01Z", "brokerVersion": "0.1.0", "engineRevision": null, "snapshot": {"kind": "read_transaction", "retained": false}},
  "evidence": {"completeness": "complete", "coverage": "recorded_only", "currentness": "not_checked"},
  "resolution": {"selectors": [{"index": 0, "pacticipant": "Checkout", "versions": ["1"], "missingReason": null, "requested": {"pacticipant": "Checkout", "version": "1", "latest": false, "tag": null, "branch": null, "mainBranch": false}}], "target": {"kind": "environment", "name": "production", "uuid": "00000000-0000-4000-8000-000000000010", "href": "/environments/00000000-0000-4000-8000-000000000010"}, "candidates": [{"id": "candidate-1", "pacticipant": "Payments", "version": "2", "selectorIndices": [0], "sources": [{"kind": "deployed", "uuid": "00000000-0000-4000-8000-000000000020", "href": "/deployed-versions/00000000-0000-4000-8000-000000000020", "applicationInstance": null, "recordedAt": "2026-11-01T09:00:00Z"}]}], "excluded": [], "ignores": []},
  "rows": [{"id": "row-1", "consumer": "Checkout", "consumerVersion": "1", "provider": "Payments", "providerVersion": "2", "candidateRefs": ["/resolution/candidates/0"], "disposition": "considered", "contract": {"hash": {"algorithm": "sha1", "value": "1234567890abcdef1234567890abcdef12345678"}, "href": "/pacts/provider/Payments/consumer/Checkout/pact-version/1234567890abcdef1234567890abcdef12345678", "publicationHref": "/pacts/provider/Payments/consumer/Checkout/version/1"}, "verification": {"number": 1, "success": false, "verifiedAt": "2026-11-01T10:00:00Z", "href": "/pacts/provider/Payments/consumer/Checkout/pact-version/1234567890abcdef1234567890abcdef12345678/verification-results/1"}}],
  "reasons": [{"code": "VERIFICATION_FAILED", "role": "decisive", "message": "Payments version 2 failed verification of the Checkout version 1 contract.", "evidenceRefs": ["/rows/0/contract", "/resolution/candidates/0", "/rows/0/verification"], "actionIds": ["action-1"]}],
  "nextActions": [{"id": "action-1", "type": "FIX_FAILED_VERIFICATION", "message": "Inspect the failed result, fix the incompatibility, and publish a new verification.", "evidenceRefs": ["/rows/0/contract", "/resolution/candidates/0", "/rows/0/verification"], "target": {"consumer": "Checkout", "consumerVersion": "1", "provider": "Payments", "providerVersion": "2", "contractSha": "1234567890abcdef1234567890abcdef12345678", "verificationHref": "/pacts/provider/Payments/consumer/Checkout/pact-version/1234567890abcdef1234567890abcdef12345678/verification-results/1"}}]
}
```

### Complete unknown example

```json
{
  "schemaVersion": "1.0",
  "decision": "unknown",
  "summary": "No verification is recorded for this contract by Payments version 2.",
  "compatibility": {"deployable": null, "success": 0, "failed": 0, "unknown": 1, "ignored": 0, "hasIgnores": false},
  "request": {"query": "pacticipant=Checkout&version=1&environment=production&latestby=cvp"},
  "evaluation": {"id": "00000000-0000-4000-8000-000000000003", "startedAt": "2026-11-01T12:00:00Z", "evaluatedAt": "2026-11-01T12:00:01Z", "brokerVersion": "0.1.0", "engineRevision": null, "snapshot": {"kind": "read_transaction", "retained": false}},
  "evidence": {"completeness": "incomplete", "coverage": "recorded_only", "currentness": "not_checked"},
  "resolution": {"selectors": [{"index": 0, "pacticipant": "Checkout", "versions": ["1"], "missingReason": null, "requested": {"pacticipant": "Checkout", "version": "1", "latest": false, "tag": null, "branch": null, "mainBranch": false}}], "target": {"kind": "environment", "name": "production", "uuid": "00000000-0000-4000-8000-000000000010", "href": "/environments/00000000-0000-4000-8000-000000000010"}, "candidates": [{"id": "candidate-1", "pacticipant": "Payments", "version": "2", "selectorIndices": [0], "sources": [{"kind": "deployed", "uuid": "00000000-0000-4000-8000-000000000020", "href": "/deployed-versions/00000000-0000-4000-8000-000000000020", "applicationInstance": null, "recordedAt": "2026-11-01T09:00:00Z"}]}], "excluded": [], "ignores": []},
  "rows": [{"id": "row-1", "consumer": "Checkout", "consumerVersion": "1", "provider": "Payments", "providerVersion": null, "candidateRefs": ["/resolution/candidates/0"], "disposition": "considered", "contract": {"hash": {"algorithm": "sha1", "value": "1234567890abcdef1234567890abcdef12345678"}, "href": "/pacts/provider/Payments/consumer/Checkout/pact-version/1234567890abcdef1234567890abcdef12345678", "publicationHref": "/pacts/provider/Payments/consumer/Checkout/version/1"}, "verification": null}],
  "reasons": [{"code": "VERIFICATION_MISSING", "role": "decisive", "message": "No verification is recorded for this contract by Payments version 2.", "evidenceRefs": ["/rows/0/contract", "/resolution/candidates/0", "/rows/0/verification"], "actionIds": ["action-1"]}],
  "nextActions": [{"id": "action-1", "type": "VERIFY_CONTRACT", "message": "Verify this exact contract with Payments version 2 and publish the result.", "evidenceRefs": ["/rows/0/contract", "/resolution/candidates/0", "/rows/0/verification"], "target": {"consumer": "Checkout", "consumerVersion": "1", "provider": "Payments", "providerVersion": "2", "contractSha": "1234567890abcdef1234567890abcdef12345678", "verificationHref": null}}]
}
```

## Reason-code catalogue (v1)

Codes are facts from the single resolution/evaluation trace. They do not constitute a
second policy engine. Emit reasons in compatibility notice/row order, with additional
selection advisories afterward. Evidence references and exact action
targets are mandatory where data exists; null is explicit when it does not.

| Code | Applies when | Suggested action |
| --- | --- | --- |
| `SELECTOR_VERSION_MISSING` | A requested selector resolves to no versions and the existing evaluator treats that as a missing-version error. Covers pins, latest, tag, branch and main branch; subtype/selector records the distinction. Decisive blocked even with unknown rows. | `RESOLVE_SELECTOR`: correct the request or publish/record the named version and required contracts/results; do not claim version creation alone ensures safety. |
| `VERIFICATION_MISSING` | A considered matrix row has no verification of its exact contract. Decisive unknown unless a missing-selector error dominates. | `VERIFY_CONTRACT` for the exact retained candidates; otherwise `RESOLVE_TARGET_VERSION`. |
| `VERIFICATION_FAILED` | A considered row's selected verification failed. Decisive blocked only without missing-selector errors or unknown rows; otherwise supporting. | `FIX_FAILED_VERIFICATION`: inspect that result, fix incompatibility and publish a new result for the exact contract/version. |
| `ALL_REQUIRED_VERIFICATIONS_SUCCESSFUL` | The shared verdict is true and success > 0. Does not assert ignored evidence succeeded. | None. |
| `NO_RECORDED_REQUIRED_ROWS` | The shared verdict is true and success=failed=unknown=0, including all rows ignored. | None; show recorded-only coverage and any ignore reasons. |
| `TARGET_VERSION_UNRESOLVED` | Resolution has no exact counterpart candidate for an unknown row (empty environment/latest/tag/any), not merely a compatibility null version with a known candidate. Supporting to verification missing. | `RESOLVE_TARGET_VERSION`: name the participant and environment/tag constraint; determine and record the intended target before verification. |
| `ROW_IGNORED` | A row is ignored by the existing rules; include the exact matched ignore request index and evidence. Advisory; its verification cannot independently block. | None; never suggest ignoring a failure as a remedy. |
| `LATESTBY_EXCLUDED` | A candidate row is removed by latestby, including null-first and equal-order cases. Reference the selected group representative and excluded evidence. Advisory. | None. |
| `RESOLUTION_EXCLUDED` | Existing resolution omits an unverified candidate because verified candidates exist, or overlapping selectors collapse duplicate evidence. Record subtype `unverified_candidate` or `duplicate`. Advisory; not a missing-verification blocker. | None. |
| `IGNORE_TARGET_NOT_FOUND` | Existing warning for unknown ignored participant or specified version. Distinguish kind in the referenced ignore request. | `CORRECT_IGNORE_REQUEST` for that request; never silently broaden it. |

The existing unpinned-selector and unspecified-environment warnings remain in the
compatibility engine, but cannot occur after can-i-deploy required-field validation;
they need no public v1 reason/action codes. Service-level matrix parity still preserves
their legacy notices.

No `CONTRACT_MISSING` code is inferred from an empty row set: the existing query cannot
prove that a service should have published a contract for an unrecorded integration.
Missing input fields or unknown participants/environments use the 400 envelope, not a
reason on a fabricated blocked/unknown decision. This prevents transport failure from
being treated as deployment success.

## Snapshot context, incomplete evidence and staleness

`evidence.completeness` is complete exactly when the evaluation has no missing selector
errors and no considered unknown rows. It means complete **for this decision and query**;
a failed verification is complete negative evidence. Ignored/excluded evidence does not
reduce this value; it remains visible separately. Coverage is always `recorded_only`.
An allowed empty result is allowed with this limitation, not recast to unknown.

The response captures a consistent point-in-time view, not a lease or assertion that
the world is still unchanged. `currentness` is always `not_checked` in v1: there is no
post-evaluation change detector, historical snapshot endpoint or global database revision.
Once deployed versions, supported releases, contracts, results, tags, branches or ignores
change, a saved explanation may no longer apply. Re-request the same query immediately
before acting. Exact pins reduce selector drift but do not freeze environment membership
or verification publication. A fresh request is a new evaluation, never a hidden second
evaluation inside the same response. Responses are not cached server-side in v1.

Return each selected verification's `verifiedAt` and the evaluation's `evaluatedAt`;
v1 does not calculate ages, apply an expiry policy, or label timestamps fresh/stale.
There is no freshness-budget parameter, classification, reason or action. Old timestamps
do not by themselves invalidate verification of immutable content. Clients may display
them, but must not interpret `currentness: not_checked` as evidence that nothing changed.
These limits must be visible in the public documentation, not just this plan.

## Schema versioning

URL `/v1/` fixes major version; `schemaVersion: "1.0"` identifies the document contract.
Publish `docs/api/deployment-explanations-v1.schema.json` and narrative documentation.
New optional fields may be added in a minor version; clients must ignore unknown object
members. The decision enum, reason-code meanings, role semantics, nullability, request
interpretation and verdict mapping cannot change within v1. Reason and action codes are
**open enums**: adding a code is a minor schema change. Clients must tolerate unknown
codes, retain/display their message and evidence where possible, and always read the
explicit decision rather than infer it from recognized codes. Unknown actions remain
non-executing suggestions. The JSON Schema must permit arbitrary string codes, with
known values documented as examples rather than a closed `enum`. Existing code meanings
remain stable; incompatible meanings or structural changes require a new major version. Messages are human text and
may improve without becoming machine contracts. Evidence IDs/JSON Pointers are local to
one response; consumers must not persist them as cross-evaluation identifiers. Timestamps
are UTC RFC3339. The schema covers success and error envelopes separately; auth middleware
and generic 404/405 errors retain their existing contracts. No new schema is retrofitted
onto Ruby-compatible responses.

## Parity and testing contract

The explanation maps the stored `DecisionEvaluation.compatibility.deployable` directly,
not from its counts, reason codes or actions. Serialize both views from one
owned result with no further DB reads. Pin a three-way mapping test and mixed-state
precedence tests so unknown cannot become true or false merely because an explanation
calls a failure a blocker. Preserve legacy notice text/order and conditional ignored key.

Reuse all **108** captured cases without modifying their expected fixtures or treating
unknown as failure. Replay setup in an isolated database, then for **every** case feed
its original matrix query directly to the shared service, obtain one evaluation and
render both the legacy matrix projection and explanation projection. Assert tri-state
mapping, counts and row identities against the same Ruby capture and resolve all typed
cause/action references. This service-level test covers all 39 true, 51 false and 18 null
cases even when their queries would not pass can-i-deploy HTTP validation. Do not add an
HTTP bypass, change a captured query to make it eligible, or expose a matrix route.

Separately, keep the existing `/matrix` capture replay and full HAL fixture tests.
For captured queries that satisfy `/can-i-deploy` validation, compare that route and the
sole explanation route against the capture on a quiescent database. Record the eligible
case set, require every eligible case to run, and add explicit flat-query/validation tests.
Queries outside that set must receive the matching validation error from both can-i-deploy
routes, not a decision. Sequential HTTP comparisons do not prove shared snapshots;
the single-evaluation service tests and concurrent WAL probe establish those contracts.

Additional tests cover known-but-unverified candidate provenance; missing pins; ignored
null versions that must remain unignored under Ruby semantics; failed+unknown precedence;
zero rows/all ignored; source records for deployments, supported releases and application
instances; exact SHA reuse across consumer versions; latestby cvp/cvpv/cp/none, first-null
and stable tie selection; escaped link components; all JSON references and action targets;
preservation of verification timestamps and evaluatedAt; and tolerance of unknown
reason/action codes without changing the explicit verdict. No action
may invent a provider version, select a different contract or execute a mutation.

The extension has no Ruby equivalent to capture. New shapes are specified and tested
locally; their verdict oracle remains Ruby's captured compatibility behavior. Before
build decisions rely on snapshot mechanics, link behavior or auth routing, probe those
exact calls on the real binary/SQLite/clients in scratch environments. Do not label new
HTTP contracts Ruby-verified. Run the unchanged real pact-broker CLI e2e suite as a
compatibility check. This planning task has not run the future tests or claimed them green.

## Decisions and rejected alternatives

| Decision | Rejected alternative and reason |
| --- | --- |
| One can-i-deploy explanation GET route | A matrix explanation route or mode switch adds unnecessary v1 surface; defer it. Adding fields/links to existing HAL/index documents risks fidelity; POST is classified as write by current auth. |
| Shared immutable evaluation; narrow #5 prerequisite | Recompute using a separate explanation resolver or parse notices: can disagree on snapshot, latestby, ignores, null identities and precedence. Waiting for all batching/performance work in #5 is unnecessary. |
| Preserve Ruby verdict and expose limitations | "Fixing" empty results or unknown+failed precedence inside explanations creates competing deployment policies. Any compatibility change requires a separate issue and real-broker evidence. |
| Keep candidate identity separate from compatibility version | Filling null matrix versions from prose changes Ruby documents and version-specific ignore semantics. |
| One transaction and fully materialized views | Rendering extra DB reads after evaluation can mix snapshots. New global revision tables or stored evaluation artifacts impose scope and lifecycle costs; defer them. |
| Deferred read-only snapshot in the separate #5 prerequisite | Db::begin uses BEGIN IMMEDIATE, blocking writers for the evidence-read duration. Pooled independent reads can mix snapshots. A real WAL concurrency probe must prove the new handle. |
| Timestamps and currentness not_checked only | A freshness budget, automatic expiry or stale=false adds v1 policy or implies recency the broker cannot establish. |
| Open reason/action enums | Closed enums force a major version for additive explanations; clients instead tolerate unknown codes while trusting the explicit decision. |
| Concrete suggested actions, no mutation | Running commands, publishing results, recording deployments, or adding ignores belongs to separately approved workflows (#13/#14), not this evidence API. |
| SHA and broker evidence links, no derived interaction index | Parsing all HTTP/message/plugin semantics duplicates #11 and is unnecessary to explain the selected contract verdict. |
| No retained evaluation/history endpoint | It would require storage, retention and authorization design; v1 returns an honest point-in-time response with no replay guarantee. |

## Maintainer decisions and remaining questions

The maintainer selected one can-i-deploy route, a separate pure-evaluator/read-snapshot
prerequisite PR, timestamps without age policy, separate candidate provenance preserving
Ruby nulls, and open reason/action enums. Those choices are settled, not open questions.
No remaining product decision blocks this revised plan. The prerequisite's internal
read-query interface and lifetime implementation will be reviewed in its own PR against
the API and concurrency acceptance criteria above. Implementation still awaits plan review.

## Ordered build tasks after approval

These are future tasks; this PR remains plan-only. Each behavioral step starts with a
failing assertion about missing behavior, then implementation, then mutation checks of
every added/changed guard. Pure refactoring uses characterization tests. Keep #11 and
broader #5 batching/performance work outside these two PRs. Do not merge automatically.

### PR A: narrow #5 prerequisite — pure evaluator and read-only snapshot

1. **Add/probe the read handle.** Files: `src/db/mod.rs` and read-query modules as needed
   to expose their existing reads through `ReadSnapshot`, without exposing writes.
   Tests in `tests/read_snapshot.rs`: `read_snapshot_does_not_block_publish`,
   `read_snapshot_does_not_block_deployment`, `snapshot_reads_all_before_or_all_after`,
   `snapshot_cleanup_on_error_and_cancellation`, `snapshot_works_with_one_connection`,
   `read_snapshot_cannot_upgrade_or_escape_to_writes`,
   `begin_read_rejects_existing_write_transaction`. Include API/type-level checks for
   the absence of mutation access and runtime checks for rejected invalid transaction
   composition. Probe file/WAL concurrency with barriers and real business writes as
   specified above; begin with tests that detect BEGIN IMMEDIATE blocking or pooled
   mixed-state reads. No concurrency claim based on the one-connection in-memory DB.
2. **Extract and characterize the shared evaluator.** Files: `src/service/matrix.rs`,
   `src/service/matrix/evaluation.rs`, `src/http/matrix.rs` and read-query modules needed
   for complete decorations. Materialize through `begin_read`, close it, evaluate once,
   then render from owned evidence. Keep typed resolution/cause provenance sufficient
   for the later explanation projection. Tests in `tests/matrix_evaluation.rs`:
   `one_evaluation_owns_verdict_and_causes`, `compatibility_renderer_needs_no_database`,
   `all_108_cases_match_extracted_evaluator`. Preserve existing
   `captured_matrix_regressions_match_ruby` and `matrix_documents_match_the_ruby_broker`.
   No explanation route or schema is introduced by this prerequisite PR.
3. **Verify and review PR A.** Mutation-check read-handle usage, transaction composition,
   cleanup, verdict precedence, ignores and latestby. Prove the writer completes while
   the snapshot remains open, then rerun compatibility checks. Run the commands below
   and a completed non-Codex code review; fix verified findings test-first. Submit the
   prerequisite separately. Only after it lands, rebase and start PR B on its accepted seam.

### PR B: #10 — explanation API using the accepted prerequisite

1. **Specify schema and examples.** Add `docs/api/deployment-explanations.md`,
   `docs/api/deployment-explanations-v1.schema.json` and
   `tests/fixtures/pactling/explanations/{allowed,blocked,unknown}.json`.
   Tests in `tests/deployment_explanations.rs`: `examples_match_versioned_schema`,
   `decision_and_error_envelopes_are_disjoint`, `unknown_reason_and_action_codes_are_valid`.
   Probe a dev-only schema validator before adding a dependency if none exists.
2. **Complete explanation provenance.** Files: shared evaluation types,
   `src/service/matrix.rs`, `src/db/matrix.rs`, `src/db/environments.rs` as needed for
   membership evidence via the read handle (no migration). Tests:
   `unverified_pin_retains_candidate_without_changing_null_row`,
   `environment_membership_identifies_deployments_releases_and_instances`,
   `latestby_exclusions_do_not_become_blockers`, `ignore_targets_match_legacy_null_rules`.
   Never change selected compatibility rows to supply missing provenance.
3. **Project explanations.** Add `src/service/deployment_explanations.rs` and register in
   `src/service/mod.rs`; accept an evaluation reference, no Db. Tests:
   `unknown_with_failure_stays_unknown`, `missing_selector_dominates_unknown`,
   `empty_and_all_ignored_results_retain_allowed`, `actions_name_exact_contract_and_version`,
   `unresolved_target_has_no_speculative_verify_action`,
   `excluded_or_ignored_rows_never_generate_verify_actions`,
   `retained_unknown_row_can_target_a_version_also_excluded_elsewhere`,
   `every_reason_reference_resolves`
   (including actions/candidate/exclusion pointers), `timestamps_are_preserved_without_age_policy`,
   `same_evaluation_has_deterministic_explanation`, `one_evaluation_feeds_both_projections`.
4. **Add the sole v1 route.** Files: `src/app.rs`, `src/http/mod.rs`, new
   `src/http/deployment_explanations.rs`; reuse can-i-deploy validation in
   `src/http/matrix.rs` without changing compatibility errors. Tests:
   `extension_maps_allowed_blocked_unknown`, `can_i_deploy_validation_matches_compatibility`,
   `can_i_deploy_query_options_and_order_are_preserved`,
   `extension_is_read_only_and_auth_protected`, `methods_and_no_store_headers`,
   `escaped_evidence_links_resolve`, `internal_errors_have_no_decision`,
   `matrix_explanation_route_is_not_exposed`. Probe real local HTTP requests, read-only
   auth, evidence links, prefix/base URL, errors and unchanged root/index documents.
5. **Enforce all 108 captured cases at the shared projection seam and eligible cases
   over HTTP.** Files: `tests/matrix_regressions.rs`, optional
   `tests/common/matrix_capture.rs`, `tests/common/mod.rs`, `tests/deployment_explanations.rs`.
   Add `shared_evaluation_projects_all_108_cases_once`,
   `explanations_match_all_108_ruby_matrix_cases` (service-level), and
   `eligible_captured_queries_match_can_i_deploy_explanation_http`. Require every case in
   the appropriate layer, expected row/null/ignore projections and the 39/51/18 totals;
   keep fixture bytes unchanged. Explicit count changes require new captures.
6. **Verify and review PR B.** Mutation-check mapping, precedence, ignores, latestby,
   candidate/null separation, action targeting and snapshot-handle usage. Document the
   extension in README's deliberate differences and public API docs, including open
   codes and currentness limits. Run the commands below and a completed non-Codex code
   review; fix verified findings test-first before pushing/opening the implementation PR.

For **each** implementation PR run `cargo fmt --check`,
`cargo clippy --all-targets -- -D warnings`, `cargo test`, and `bash e2e/run.sh`
(install e2e dependencies with `npm ci` if needed). Run
`agent-review code main..HEAD` with a non-Codex reviewer. Commits remain Conventional
Commits without attribution. Plan approval precedes build; separate maintainer approval
is required to merge either PR. No automatic release.

## Risks and boundaries of proof

The main risks are loss of resolution provenance, treating ignored/excluded candidates as
new blockers, mixing snapshots during HTTP decoration, accidentally retaining a write
lock in the read path, and long readers delaying WAL checkpoints. The 108 captures
constrain decisions but do not prove the read handle's isolation, concurrency or cleanup. New tests/probes must cover those explicitly. Keeping exact
source/fixture-backed behavior separate from proposed API policy is part of acceptance.
A later change to a verdict, a freshness policy that blocks deployment, or an expansion
into #11 must return to maintainer design review rather than slipping into this build.
