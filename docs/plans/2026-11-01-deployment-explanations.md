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
verdict, including counterintuitive Ruby precedence, expose the limits and age of the
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
in this plan PR. Have the maintainer approve/land that narrow prerequisite separately;
then rebase and adapt this plan to its accepted types. If #5 lands in full first, reuse it.

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

Snapshot proposal for the prerequisite: initially use the existing `Db::begin` owner,
load all required evidence/decorations through it, materialize the decision, finish the
transaction, then render. This includes every lookup currently in `version_document`
(branches, environments, tags and tag-latest checks). Test a large matrix as well as
a single row: the per-version reads can extend the writer lock substantially; record
query count, matrix size and writer wait time in the prerequisite probe results. No network calls or JSON serialization while holding its lock.
This intentionally accepts short-lived writer contention instead of designing a new
transaction API inside #10. Before accepting the prerequisite, probe with the real pinned
sqlx/SQLite versions on a disposable file database: concurrent publish/deploy/release
must appear entirely before or after the evaluation, never mixed. Check writer waits,
lock release on failures, and the in-memory single-connection path. If contention is
unacceptable, review a deferred read transaction design under #5; do not silently fall
back to pooled reads or claim snapshot consistency. No snapshot identifier promises
historical replay, and this API does not reserve permission to deploy.

## Proposed API and request contract

Two read-only extension routes:

- `GET /pactling/v1/matrix/explanation?<existing matrix query>`
- `GET /pactling/v1/can-i-deploy/explanation?<existing can-i-deploy query>`

Both return `application/json`, status 200 for all three valid decisions, and
`Cache-Control: no-store`. `GET, OPTIONS` are supported; other methods receive 405 with
Allow. Use existing authentication/read-access middleware, including configured public
reads and read-only credentials. No auth bypass or new root HAL relation. Existing
routes, response bodies, content types, links, notices and index documents remain unchanged.
The extension is documented in README's deliberate differences and a dedicated API doc.

The matrix route accepts the same raw ordered query grammar through `MatrixQuery::parse`;
array order and duplicates are preserved. The can-i-deploy route uses the **same** required
field validation as its compatibility counterpart. Do not introduce new defaults or
stricter matrix validation only on the extension. Unsupported query keys retain current
parser behavior. Errors such as unknown participant/environment are request errors,
not unknown decisions: preserve counterpart status and validation semantics, with a
versioned extension JSON envelope `{ "schemaVersion": "1.0", "error": {
"code": "INVALID_QUERY", "details": <existing errors value> } }` for 400.
Authentication responses remain owned by the existing middleware; unsupported versions
remain 404; internal failures are 500 with no decision and no internal detail.

One optional extension parameter, `evidenceMaxAgeSeconds`, is removed before parsing the
compatibility query. If present, it must occur once and, after URL decoding, consist only of one or more
ASCII decimal digits, with numeric value in the inclusive range 0..2147483647 (leading
zeros allowed). Empty strings, signs, whitespace, fractions, exponent syntax, non-digits,
overflow, out-of-range and repeated values produce 400 `INVALID_QUERY`. Omission means no age policy. It affects
only freshness annotations, **never** verdict, rows, counts or selection. The response
records the original compatibility query (without this parameter), its route mode,
and the age policy. Clients must URL-encode values; a JSON request body is not accepted.
For example:

```text
GET /pactling/v1/can-i-deploy/explanation?pacticipant=Checkout&version=1&environment=production&latestby=cvp&evidenceMaxAgeSeconds=86400
```

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
  evidence is materialized. `snapshot.kind = transaction` describes isolation, not durability.
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
  Non-dominant row facts remain supporting; ignored/excluded and freshness warnings
  are advisory. All messages are deterministic templates, with no prose parsing.
- `nextActions` are suggestions only: id, type, message, evidenceRefs and typed `target`.
  Deduplicate by action type and exact target; order by first reason occurrence. A verify
  action names consumer/provider versions and contract SHA, a fix action names the failed
  result, and missing selection suggests recording/publishing or correcting that exact
  request. Only candidates contributing to a considered unknown row can be required verification
  targets; excluded and ignored candidates never generate remediation work. If that row
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
  "request": {"mode": "can-i-deploy", "query": "pacticipant=Checkout&version=1&environment=production&latestby=cvp", "evidenceMaxAgeSeconds": 86400},
  "evaluation": {"id": "00000000-0000-4000-8000-000000000001", "startedAt": "2026-11-01T12:00:00Z", "evaluatedAt": "2026-11-01T12:00:01Z", "brokerVersion": "0.1.0", "engineRevision": null, "snapshot": {"kind": "transaction", "retained": false}},
  "evidence": {"completeness": "complete", "coverage": "recorded_only", "currentness": "not_checked", "freshness": {"status": "within_budget", "maxAgeSeconds": 86400, "oldestVerificationAt": "2026-11-01T10:00:00Z", "ageExceededRefs": [], "unavailableRefs": []}},
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
  "request": {"mode": "can-i-deploy", "query": "pacticipant=Checkout&version=1&environment=production&latestby=cvp", "evidenceMaxAgeSeconds": 86400},
  "evaluation": {"id": "00000000-0000-4000-8000-000000000002", "startedAt": "2026-11-01T12:00:00Z", "evaluatedAt": "2026-11-01T12:00:01Z", "brokerVersion": "0.1.0", "engineRevision": null, "snapshot": {"kind": "transaction", "retained": false}},
  "evidence": {"completeness": "complete", "coverage": "recorded_only", "currentness": "not_checked", "freshness": {"status": "within_budget", "maxAgeSeconds": 86400, "oldestVerificationAt": "2026-11-01T10:00:00Z", "ageExceededRefs": [], "unavailableRefs": []}},
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
  "request": {"mode": "can-i-deploy", "query": "pacticipant=Checkout&version=1&environment=production&latestby=cvp", "evidenceMaxAgeSeconds": 86400},
  "evaluation": {"id": "00000000-0000-4000-8000-000000000003", "startedAt": "2026-11-01T12:00:00Z", "evaluatedAt": "2026-11-01T12:00:01Z", "brokerVersion": "0.1.0", "engineRevision": null, "snapshot": {"kind": "transaction", "retained": false}},
  "evidence": {"completeness": "incomplete", "coverage": "recorded_only", "currentness": "not_checked", "freshness": {"status": "unavailable", "maxAgeSeconds": 86400, "oldestVerificationAt": null, "ageExceededRefs": [], "unavailableRefs": ["/rows/0/verification"]}},
  "resolution": {"selectors": [{"index": 0, "pacticipant": "Checkout", "versions": ["1"], "missingReason": null, "requested": {"pacticipant": "Checkout", "version": "1", "latest": false, "tag": null, "branch": null, "mainBranch": false}}], "target": {"kind": "environment", "name": "production", "uuid": "00000000-0000-4000-8000-000000000010", "href": "/environments/00000000-0000-4000-8000-000000000010"}, "candidates": [{"id": "candidate-1", "pacticipant": "Payments", "version": "2", "selectorIndices": [0], "sources": [{"kind": "deployed", "uuid": "00000000-0000-4000-8000-000000000020", "href": "/deployed-versions/00000000-0000-4000-8000-000000000020", "applicationInstance": null, "recordedAt": "2026-11-01T09:00:00Z"}]}], "excluded": [], "ignores": []},
  "rows": [{"id": "row-1", "consumer": "Checkout", "consumerVersion": "1", "provider": "Payments", "providerVersion": null, "candidateRefs": ["/resolution/candidates/0"], "disposition": "considered", "contract": {"hash": {"algorithm": "sha1", "value": "1234567890abcdef1234567890abcdef12345678"}, "href": "/pacts/provider/Payments/consumer/Checkout/pact-version/1234567890abcdef1234567890abcdef12345678", "publicationHref": "/pacts/provider/Payments/consumer/Checkout/version/1"}, "verification": null}],
  "reasons": [{"code": "VERIFICATION_MISSING", "role": "decisive", "message": "No verification is recorded for this contract by Payments version 2.", "evidenceRefs": ["/rows/0/contract", "/resolution/candidates/0", "/rows/0/verification"], "actionIds": ["action-1"]}],
  "nextActions": [{"id": "action-1", "type": "VERIFY_CONTRACT", "message": "Verify this exact contract with Payments version 2 and publish the result.", "evidenceRefs": ["/rows/0/contract", "/resolution/candidates/0", "/rows/0/verification"], "target": {"consumer": "Checkout", "consumerVersion": "1", "provider": "Payments", "providerVersion": "2", "contractSha": "1234567890abcdef1234567890abcdef12345678", "verificationHref": null}}]
}
```

## Reason-code catalogue (v1)

Codes are facts from the single resolution/evaluation trace. They do not constitute a
second policy engine. Emit reasons in compatibility notice/row order, with additional
selection and freshness advisories afterward. Evidence references and exact action
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
| `UNPINNED_SELECTOR` | Existing version-number reliability warning condition: some selector is constrained, but none supplies a version number. Keep that condition rather than inventing stricter compatibility policy. | `PIN_RESOLVED_VERSIONS` with the actual selector resolutions. |
| `ENVIRONMENT_UNSPECIFIED` | Existing warning condition: one selector, no environment, no tag. | `SPECIFY_TARGET_ENVIRONMENT`; requires user choice, not an invented target. |
| `EVIDENCE_AGE_EXCEEDED` | At least one considered selected verification is older than the explicitly requested age budget. Advisory only, with timestamp references. | `RECHECK_EVIDENCE`: request an updated evaluation/verification as appropriate; do not claim rerunning the query alone refreshes a verification. |
| `EVIDENCE_TIME_UNAVAILABLE` | A considered verification timestamp is absent, unparsable or later than evaluatedAt; age cannot be assessed for it. Missing verification itself is already explained by VERIFICATION_MISSING and does not duplicate this reason. | `CHECK_EVIDENCE_TIMESTAMP`; no invented age or clock correction. |

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

Age-based staleness is reported separately and conservatively. Compare evaluatedAt against
selected, considered verification timestamps only; old publication/deployment timestamps
do not mean their facts expired. `age > evidenceMaxAgeSeconds` exceeds the budget; equality
does not. No age budget yields freshness `not_checked`. With a budget: any exceeded age
sets `age_exceeded`, else any missing verification/invalid timestamp sets `unavailable`,
else `within_budget`. Zero considered rows yields `not_checked` (no evidence to age).
`ageExceededRefs` and `unavailableRefs` are populated even when another status wins;
`oldestVerificationAt` is the oldest valid considered verification timestamp, else null.
An expired budget or unknown age never changes deployable. `within_budget` does not mean
unchanged since evaluation. The API explicitly cannot detect unrecorded real-world changes.
These limits must be visible in the public documentation, not just this plan.

## Schema versioning

URL `/v1/` fixes major version; `schemaVersion: "1.0"` identifies the document contract.
Publish `docs/api/deployment-explanations-v1.schema.json` and narrative documentation.
New optional fields may be added in a minor version; clients must ignore unknown object
members. The decision enum, reason-code meanings, role semantics, nullability, request
interpretation and verdict mapping cannot change within v1. Adding reason/action enum
values requires a new major version in this initial closed-enum design; maintainers may
choose a documented open-enum policy before implementation. Messages are human text and
may improve without becoming machine contracts. Evidence IDs/JSON Pointers are local to
one response; consumers must not persist them as cross-evaluation identifiers. Timestamps
are UTC RFC3339. The schema covers success and error envelopes separately; auth middleware
and generic 404/405 errors retain their existing contracts. No new schema is retrofitted
onto Ruby-compatible responses.

## Parity and testing contract

The explanation maps the stored `DecisionEvaluation.compatibility.deployable` directly,
not from its counts, reason codes, freshness or actions. Serialize both views from one
owned result with no further DB reads. Pin a three-way mapping test and mixed-state
precedence tests so unknown cannot become true or false merely because an explanation
calls a failure a blocker. Preserve legacy notice text/order and conditional ignored key.

Reuse all **108** captured cases, without modifying expected fixtures or reinterpreting
unknown as failure. Refactor setup replay into `tests/common/matrix_capture.rs` if needed:
replay setup once in an isolated server, then for every captured request assert the
legacy status, summary and row projection against the capture, and the extension's
status, tri-state mapping, counts and row identities against the **same expected capture**.
The matrix extension handles every recorded query. Also feed its query through the
shared service to render both views from one evaluation (not two calls) and assert typed
cause/action references resolve. Compare can-i-deploy and its extension for requests that
satisfy its required-field validation; add separate validation/flat-query tests for it.
Sequential HTTP comparisons run on a quiescent test database, not as proof that two live
requests share a snapshot. Keep the existing full-document fixture tests unchanged.

Additional tests cover known-but-unverified candidate provenance; missing pins; ignored
null versions that must remain unignored under Ruby semantics; failed+unknown precedence;
zero rows/all ignored; source records for deployments, supported releases and application
instances; exact SHA reuse across consumer versions; latestby cvp/cvpv/cp/none, first-null
and stable tie selection; escaped link components; all JSON references and action targets;
and freshness equality, exceedance, omission, missing/invalid/future timestamps. No action
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
| Extension GET routes, one for each existing query mode | Adding fields or links to existing HAL/index documents risks drop-in fidelity; POST would also be classified as write by current auth. |
| Shared immutable evaluation; narrow #5 prerequisite | Recompute using a separate explanation resolver or parse notices: can disagree on snapshot, latestby, ignores, null identities and precedence. Waiting for all batching/performance work in #5 is unnecessary. |
| Preserve Ruby verdict and expose limitations | "Fixing" empty results or unknown+failed precedence inside explanations creates competing deployment policies. Any compatibility change requires a separate issue and real-broker evidence. |
| Keep candidate identity separate from compatibility version | Filling null matrix versions from prose changes Ruby documents and version-specific ignore semantics. |
| One transaction and fully materialized views | Rendering extra DB reads after evaluation can mix snapshots. New global revision tables or stored evaluation artifacts impose scope and lifecycle costs; defer them. |
| Existing BEGIN IMMEDIATE first, gated by contention probe | Unproven deferred-read API inside #10 would expand #5 scope; unconstrained pooled reads cannot support a snapshot claim. |
| Explicit age annotation, never a verdict TTL | Implicit expiration or `stale=false` would imply safety/recency the broker cannot establish. An old successful verification of immutable content is not automatically invalid. |
| Concrete suggested actions, no mutation | Running commands, publishing results, recording deployments, or adding ignores belongs to separately approved workflows (#13/#14), not this evidence API. |
| SHA and broker evidence links, no derived interaction index | Parsing all HTTP/message/plugin semantics duplicates #11 and is unnecessary to explain the selected contract verdict. |
| No retained evaluation/history endpoint | It would require storage, retention and authorization design; v1 returns an honest point-in-time response with no replay guarantee. |

## Open questions for the maintainer

1. Approve the two `/pactling/v1/.../explanation` GET routes and existing query grammar,
   or prefer one extension route with an explicit matrix/can-i-deploy mode?
2. Should the narrow #5 prerequisite land separately first, as proposed? Is short-lived
   BEGIN IMMEDIATE writer contention acceptable if the probe passes, or should #5 first
   add a dedicated read-snapshot API?
3. Is honest `currentness: not_checked` plus optional age-budget reporting sufficient for
   v1, or is detecting changes to a previously returned evaluation required? The latter
   needs a separately designed revision/retention protocol; it is not promised here.
4. Approve null compatibility provider versions with separate exact candidate provenance,
   and allowed empty/all-ignored results with explicit recorded-only coverage?
5. Should reason/action enums be closed per major version as proposed, or open for additive
   codes? Are the catalogue and action granularity suitable for the first client?

## Ordered build tasks after approval

These are future tasks, not permission to implement in this PR. First agree the dependency
boundary; do not implement #5, #11 or #17 incidentally. Test names below are proposed
unless explicitly called existing. Each behavioral step starts with a failing assertion
about the absent behavior (not an import/compile failure), then implementation, then
mutation checks on every new/changed guard. Preserve the red-test commit unchanged in
its green commit. Pure refactoring uses characterization tests as its oracle.

1. **Land/reuse the separately approved narrow #5 seam.** Proposed files:
   `src/service/matrix.rs`, `src/service/matrix/evaluation.rs`, `src/db/mod.rs`, and
   `src/http/matrix.rs` (materialize decorations in the shared result); any dependency
   scope change is reviewed there. Tests in `tests/matrix_evaluation.rs`:
   `one_evaluation_feeds_both_projections`, `renderers_need_no_database`,
   `snapshot_never_mixes_concurrent_publication_and_deployment`,
   `snapshot_releases_lock_on_error`, `snapshot_works_with_one_connection`.
   Preserve existing `captured_matrix_regressions_match_ruby` and full HAL fixtures.
   Probe and record contention/snapshot evidence before choosing transaction mechanics.
   This is a release blocker for #10, not an instruction to complete all of #5.
2. **Specify the extension's schema and examples.** Add
   `docs/api/deployment-explanations.md`, `docs/api/deployment-explanations-v1.schema.json`,
   and `tests/fixtures/pactling/explanations/{allowed,blocked,unknown}.json`.
   Add `tests/deployment_explanations.rs::examples_match_versioned_schema` and
   `decision_and_error_envelopes_are_disjoint`. Reuse an existing schema validator if
   available; otherwise probe a chosen dev-only validator before adding it to Cargo.toml.
   These example/validator tests can characterize the contract before endpoint work.
3. **Retain typed resolution/evidence provenance on the approved seam.** Files:
   `src/service/matrix.rs`, its evaluation types, `src/db/matrix.rs` and
   `src/db/environments.rs` only as needed to load membership evidence in the same
   transaction (no storage migration). Tests in `tests/deployment_explanations.rs`:
   `unverified_pin_retains_candidate_without_changing_null_row`,
   `environment_membership_identifies_deployments_releases_and_instances`,
   `latestby_exclusions_do_not_become_blockers`, `ignore_targets_match_legacy_null_rules`.
   Fail on absent provenance first; never alter selected compatibility rows to satisfy it.
4. **Project deterministic explanations and advisory freshness.** Add
   `src/service/deployment_explanations.rs` and register in `src/service/mod.rs`.
   Accept an evaluation by reference, no Db parameter. Tests:
   `unknown_with_failure_stays_unknown`, `missing_selector_dominates_unknown`,
   `empty_and_all_ignored_results_retain_allowed`, `actions_name_exact_contract_and_version`,
   `unresolved_target_has_no_speculative_verify_action`, `freshness_never_changes_verdict`,
   `age_budget_boundary_and_unavailable_times`, `every_reason_reference_resolves` (traverse reasons and nextActions, plus candidate
   and exclusion pointers, checking each against its own response),
   `same_evaluation_has_deterministic_explanation`. Test missing typed causes and all code
   branches; never use human-readable descriptions as data.
5. **Add extension routing and rendering only.** Files: `src/app.rs`, `src/http/mod.rs`,
   new `src/http/deployment_explanations.rs`; reuse/refactor can-i-deploy validation in
   `src/http/matrix.rs` without changing its errors. Tests in the same integration file:
   `extension_maps_allowed_blocked_unknown`, `can_i_deploy_validation_matches_compatibility`,
   `matrix_query_options_and_order_are_preserved`, `invalid_age_budget_is_rejected`,
   `extension_is_read_only_and_auth_protected`, `methods_and_no_store_headers`,
   `escaped_evidence_links_resolve`, `internal_errors_have_no_decision`.
   Use real requests to the local binary, configured read-only auth and existing evidence
   endpoints; verify 405/OPTIONS/400 behavior, prefix/base URL and no added index fields.
6. **Enforce the full captured parity corpus.** Files: `tests/matrix_regressions.rs`,
   optional new `tests/common/matrix_capture.rs`, `tests/common/mod.rs`, and
   `tests/deployment_explanations.rs`. Add `explanations_match_all_108_ruby_matrix_cases`
   and `shared_evaluation_projects_all_108_cases_once`. Assert count=108, every case runs,
   expected row/null/ignore projections and all 39/51/18 verdicts; leave fixture bytes
   unchanged. Future new captures should update the explicit count intentionally.
   Run existing `tests/matrix.rs` and every other fixture suite as well.
7. **Mutation-check, document and independently review the build.** Break and restore
   verdict mapping, missing-before-unknown precedence, ignore matching, latestby selection,
   candidate/null separation, age threshold and no-budget guards, action targeting, and
   snapshot-handle use. Each must fail a relevant test. Update README deliberate differences
   and the API documentation. Run `cargo fmt --check`,
   `cargo clippy --all-targets -- -D warnings`, `cargo test`, and `bash e2e/run.sh`
   (install e2e dependencies with `npm ci` if needed). Run completed non-Codex
   `agent-review code main..HEAD`; probe findings and fix verified ones test-first.
   Commit as Conventional Commits without attribution, push a build branch and open a PR
   only after the maintainer has approved the plan. Do not merge or release automatically.

## Risks and boundaries of proof

The main risks are loss of resolution provenance, treating ignored/excluded candidates as
new blockers, mixing snapshots during HTTP decoration, and writer contention from the
initial transaction choice. The 108 captures constrain decisions but are not proof of all
concurrency or age behavior. New tests/probes must cover those explicitly. Keeping exact
source/fixture-backed behavior separate from proposed API policy is part of acceptance.
A later change to a verdict, a freshness policy that blocks deployment, or an expansion
into #11 must return to maintainer design review rather than slipping into this build.
