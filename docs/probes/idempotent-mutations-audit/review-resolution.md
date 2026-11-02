# Plan review resolutions

Initial review completed with `Reviewed by: claude sonnet` and `<<AGENT-REVIEW-END>>`.
Default Claude exhausted credits; automatic remote fallback completed. No local model used.
Findings are design claims, not newly verified production behavior. No implementation was
written to resolve them.

| Finding | Verification / resolution |
| --- | --- |
| Global permanent key cap can be exhausted by any writer | True by the proposed cardinality rule; made explicit as capacity risk and maintainer question. Not described as DoS isolation. Reject suggested finite tombstone TTL: after deletion, the same old key would be absent and execute again, contradicting the delayed-retry guarantee. `http/auth.rs::granted` exposes one RW Basic and one shared bearer role, while open mode accepts all callers; a per-principal quota cannot isolate those shared callers. No nonexistent tenant separation is claimed. Current writer trust already permits unbounded domain creation; operator rate/cap planning is mandatory. |
| Deterministic build key expires permanently | True, intentionally410 after expiry. Replaced the example with a random UUID; distinguished a new logical run from retrying its transport. Deterministic schemes need run-attempt identity; retries retain the original key. |
| Shared credential callers can discover/replay a known same-namespace key | True; source auth has no separate caller identity. Explicitly documented; minimal receipts contain no payload. Recommended unpredictable keys avoid accidental collisions, not authorization. Added shared-namespace characterization test; cross-principal isolation claim is narrowed accordingly. |
| Global auth makes new JSON401/403 unreachable | Confirmed `app.rs:178` applies `auth::require` globally, and `http/auth.rs::require` returns empty401 before handlers. Plan now splits compatibility and new mutation/audit routers with distinct auth layers before merging; existing explanation/UI remain under original auth. Added contrasting legacy401/new403 test. |
| Mandatory audit can fail a legacy write | True, a new storage-failure surface. Added explicit fail-closed deliberate difference, README requirement and legacy fault-injection test. Best-effort audit is rejected: it would commit an unaudited mutation contrary to the requested atomicity. Normal responses/auth/parser behavior stay protected by existing fixtures. |
| Time retention does not hard-bound audit volume | True. Stated burst/unkeyed-unchanged growth, moved latency/bytes measurements earlier to Task3, and added explicit operator rate/retention question. No hard bound is claimed. Silently dropping recent audit rows or skipping unchanged facts is not an assumed fix; a count/byte retention policy requires acceptance and visible truncation metadata. |
| History sequence could reuse IDs | Correct; migration now explicitly requires INTEGER PRIMARY KEY AUTOINCREMENT and a prune-all/reinsert test. Disposable Python SQLite3.54.1 probe: ordinary INTEGER PRIMARY KEY allocated1 then1 after deletion; AUTOINCREMENT allocated1 then2. This is a separate-version reproduction, not a claim it exercised SQLx. Target libsqlite3-sys0.37.0 bundled sqlite3.c OP_NewRowid source around100888 documents the largest-previously-generated register used by AUTOINCREMENT. |
| Replays unnecessarily take the writer lock | Confirmed original plan plus `Db::begin` BEGIN IMMEDIATE. Added begin_read fast path; release snapshot before write begin; recheck key under writer lock after a miss. Added writer-progress and read-miss/write-race tests. Exact repository `begin_read` source and existing `tests/read_snapshot.rs` provide the seam (those tests were read, not newly run here). |
| No credential downgrade/revocation replay test | Added restart tests with auth still enabled: revoked401, downgraded403, no receipt disclosure. Turning all auth off is intentionally open anonymous mode and cannot be advertised as revocation. |

The finite storage/trust-boundary policy choices above remain explicit questions for owner
acceptance; they are not silently resolved by adopting a conflicting retry contract.

Second completed review (`claude sonnet`, end marker present) found no critical contradiction
but requested six clarifications; all are now explicit:

- Legacy correlation is always generated server-side; incoming header ignored without new
  response headers/errors. Facade validation remains isolated. Added legacy garbage-header test.
- Capacity rejects every new key, including keyed ensures; unkeyed ensures remain available.
- First keyed outcome `unchanged` is201, replay200; `replayed` is authoritative.
- A transactionally updated singleton capacity counter avoids COUNT-scanning the growing
  ledger on each write; rollback/pruning/million-row query-plan tests are named.
- Fingerprinting is over parsed JSON Value numbers, not raw lexemes. **Verified against exact
  serde_json1.0.154 with preserve_order, in a disposable Rust crate:** `1 => 1`, `1.0 => 1.0`,
  `1e0 => 1.0`, `1.00 => 1.0`; nested duplicates keep the last value under the ordinary parser.
  See `number-probe.txt`. The review's assertion that distinguishing1 from1.0 requires
  arbitrary_precision was not borne out. The plan instead specifies a facade-only recursive
  duplicate-checking Deserialize visitor, ordinary numeric semantics, and frozen digest tests;
  no crate-wide feature or compatibility parser change. Scratch cargo clean removed25.5MiB.
- Stored fingerprint version selects the historical algorithm applied to the incoming request;
  historic vectors stay compiled/tested. Unknown versions have a distinct503 error and cannot
  execute. No original secret-bearing payload is needed for this comparison.

Third complete review (`Reviewed by: claude sonnet`, `<<AGENT-REVIEW-END>>`) confirmed all six
ambiguities resolved, reported no blocking issue, and classified remaining findings as small.
Those four clarifications were folded in before commit:

- Separate413 RESULT_TOO_LARGE from pre-work request bounds: a complete receipt can exceed
  64KiB for a large-identifier batch; all staged effects/key/audit roll back, no truncated refs
  and no automatic retry. This additional acceptance limit and a max-batch rollback test
  are explicit, rather than pretending100 contracts always fit.
- The guarantee is scoped to a principal namespace. Auth-mode changes and Basic username
  changes require reconciliation; the same key under another namespace is a new intention.
- Facade duplicate-key rejection is an explicit deliberate difference from legacy Value
  parsing, with a contrasting put-pact test and README requirement.
- Uncertain commit is resolved by retrying the same key under the same principal, not by a
  nonexistent public lookup-by-key endpoint.

Only maintainer policy/acceptance questions remain; no implementation or test-pass claim is
made for the proposed API. Review run directories are local `.git/agent-review/` records.
