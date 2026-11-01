# Deployment explanations (v1)

`GET /pactling/v1/can-i-deploy/explanation` is a read-only pactling extension. It explains the same decision as `/can-i-deploy`; it does not introduce a deployment policy. Existing Ruby-compatible resources, HAL links and documents are unchanged. There is no matrix explanation endpoint in v1.

## Request and transport

Use the existing can-i-deploy query: a pacticipant and version, plus an environment or `to` tag. Ordered selectors, duplicate selectors, `latestby`, ignores and other accepted query options retain their existing parsing and validation. `to` means latest in that tag. Unsupported keys retain the existing parser behavior. There is no request body or age-budget parameter.

```http
GET /pactling/v1/can-i-deploy/explanation?pacticipant=Checkout&version=1&environment=production&latestby=cvp
```

Valid allowed, blocked and unknown decisions all return HTTP 200, `Content-Type: application/json`, and `Cache-Control: no-store`. GET and OPTIONS are supported; other methods return the existing 405 response with Allow. The route uses the existing authentication and read-access middleware, including read-only credentials and configured public reads. Authentication failures retain that middleware's existing response. Unknown route versions return the existing 404.

Missing required fields and unknown participants or environments are request errors, not unknown deployment decisions. HTTP 400 preserves the existing validation details in this extension envelope:

```json
{"schemaVersion":"1.0","error":{"code":"INVALID_QUERY","details":{"pacticipant":["is missing"],"version":["is missing"],"":["Must specify either an environment or a 'to' tag."]}}}
```

Internal failures return HTTP 500 without a decision or internal details. Clients must check status and envelope before using a decision. Error responses do not imply allowed deployment.

## Document and evidence

The [JSON Schema](deployment-explanations-v1.schema.json) defines required fields, explicit nulls and typed targets. Complete illustrative examples: [allowed](../../tests/fixtures/pactling/explanations/allowed.json), [blocked](../../tests/fixtures/pactling/explanations/blocked.json), and [unknown](../../tests/fixtures/pactling/explanations/unknown.json). They describe separate database states and are not Ruby captures.

`decision` maps directly from `compatibility.deployable`: true → allowed, false → blocked, null → unknown. Compatibility counts come from the same immutable evaluation as the compatibility document. Reasons and actions are projections of its trace, never another evaluation. Missing selector errors take precedence over unknown rows; unknown rows take precedence over failed rows. Ignored and excluded rows cannot block. `ignored` is always present here, including zero.

`resolution` preserves selectors in request order, resolved versions, typed requested selectors, target constraints, ignore requests and exact candidate identities. Environment candidates retain deployment/release source UUIDs, timestamps, application instances and navigation links. Multiple records can select one candidate. Compatibility rows preserve Ruby's null versions: an unverified row may have `providerVersion: null` while its `candidateRefs` identify an exact known version. Null alone therefore does not mean the target is unresolved. Candidates are counterpart versions: in provider-side queries they can identify consumers. Action construction uses the retained row’s preserved provider identity; it does not assume every candidate reference identifies a provider.

`rows` retain compatibility order. Each has a local ID, disposition, contract SHA-1 and links, and a selected verification or explicit null. `resolution.excluded` contains entries with `kind` (`latestby`, `duplicate`, or `unverified_candidate`), complete `evidence` rows with disposition `excluded`, candidate references, selector indices and a nullable `selectedRowRef`. These are provenance only and do not contribute to counts. Reasons reference selected and excluded evidence without reviving excluded verification work.

`evidenceRefs`, `candidateRefs`, and `selectedRowRef` are JSON Pointers into this response, with standard `~0`/`~1` escaping. IDs and pointers are local to one response, not durable cross-evaluation identifiers. Public hrefs may be relative or absolute and are navigation aids; records may later be deleted. The response does not contain raw contracts or credentials. Absolute links use the existing `Host` and `X-Forwarded-Proto` handling; `X-Forwarded-Prefix` is not supported.

## Reasons and suggested actions

Codes are facts, not an alternate policy engine. `role` is decisive, supporting or advisory. Reasons include human messages, evidence references and action IDs. Reasons/actions follow deterministic evidence order; identical action type and exact target are deduplicated. Actions suggest work; they never execute commands, publish, deploy, ignore or retry automatically.

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
| `RESOLUTION_EXCLUDED` | Existing resolution omits an unverified candidate because verified candidates exist or retains only the newest candidate when none verified; overlapping selectors can also collapse duplicate evidence. Record subtype `unverified_candidate` or `duplicate`. Advisory; not a missing-verification blocker. | None. |
| `IGNORE_TARGET_NOT_FOUND` | Existing warning for unknown ignored participant or specified version. Distinguish kind in the referenced ignore request. | `CORRECT_IGNORE_REQUEST` for that request; never silently broaden it. |

`VERIFY_CONTRACT` and `FIX_FAILED_VERIFICATION` targets identify consumer/version, provider/version, exact contract SHA and a nullable verification URL. Only considered unknown rows and their contributing candidates generate verification work. A missing exact counterpart produces `RESOLVE_TARGET_VERSION`, with environment/tag constraints, before verification is suggested. `RESOLVE_SELECTOR` identifies the selector index and typed request; `CORRECT_IGNORE_REQUEST` identifies the ignore index, participant and optional version. No action recommends ignoring a failure. An empty recorded matrix does not prove a missing contract or complete system coverage.

## Snapshot and currentness limits

Evidence is read through a deferred read-only transaction. On file databases under WAL, it observes one consistent SQLite snapshot while allowing publishers and deployment writers to proceed. The in-memory database uses one pooled connection, so concurrent operations wait for the read transaction to end. The transaction closes after materialization; serialization uses its immutable evaluation. No retained snapshot or historical replay endpoint is created.

`evaluation.id` is a request-local UUID, not a database revision. `startedAt` is captured before snapshot acquisition; `evaluatedAt` after evidence is materialized. Broker version and optional engine revision identify the evaluator. Verification `verifiedAt` and evaluation timestamps are UTC RFC3339. No age or expiry policy is applied.

`evidence.completeness` is complete exactly when there are no missing selector errors and no considered unknown rows. A failed verification is complete negative evidence; ignored/excluded evidence does not reduce completeness. Coverage is always `recorded_only`, including an allowed empty matrix.

`currentness` is always `not_checked`. A consistent snapshot is not a lease: contracts, results, deployments, releases, branches or tags may change immediately afterward. There is no post-evaluation change detector, global database revision, or server-side response cache. Re-request immediately before acting; exact pins do not freeze environment membership or future verifications. Old timestamps alone do not invalidate verification of immutable content. There is no freshness budget, freshness classification, or freshness action in v1.

## Versioning and client behavior

The URL fixes major version 1; `schemaVersion: "1.0"` identifies this contract. Clients must ignore additional object members. Reason and action codes are **open enums**: tolerate unknown nonempty strings, retain/display their message and evidence where possible, and read the explicit decision rather than infer a verdict from known codes. Unknown actions remain non-executing suggestions. Adding a code or optional field is a minor change. The schema intentionally permits unknown codes and action target objects.

Existing code meanings, decision mapping, role semantics, nullability and request interpretation remain stable within v1. Incompatible structural or semantic changes require a new major URL version. Messages are human text and may improve; they are not machine contracts. The schema covers decision and validation-error envelopes separately. Authentication and generic 404/405 responses retain their existing contracts; no extension schema is imposed on Ruby-compatible resources.
