# Matrix evaluator prerequisite (PR A)

Implementation of the approved PR A section of
`docs/plans/2026-11-01-deployment-explanations.md` on
`origin/feat/deployment-explanations`, with the maintainer's query_only override.
Base: main `23b25d6`. No explanation endpoint, schema, README or fixture changes.

## Decisions

- Use a deferred read snapshot and SQLite `query_only` on its connection, rather than
  splitting every Db method into read/write traits. Reject nested transactions,
  write upgrades, raw pool access and migrations on a read handle.
- Reset rollback/query_only before returning the same pooled connection. Own cleanup
  through cancellation; close an uncertain connection rather than pool it. Preserve
  the sole in-memory connection by avoiding SQLx's cancellable acquire-time ping there.
- Load rows and distinct version decorations in the snapshot, close it, then run one
  pure evaluator. The HTTP document renderer takes only owned evidence and Links.
  No cache, query batching redesign, pool tuning for file databases or second evaluator.
- Keep typed missing-selector/ignore-warning causes and per-row outcomes, as well as
  exact provider candidates where Ruby renders a null version. Preserve wording,
  ordering, nulls, ignore matching and latestby behavior. Broader explanation-only
  membership/exclusion annotations belong to PR B.

## Evidence and verification

- Real SQLx 0.9.0 / bundled SQLite 3.51.3 probe establishes deferred BEGIN, read-only
  error code 8, reset/explicit pool return and exact no-active-transaction rollback
  behavior. Exact SQLx source explains cancellation of acquire-time connection testing.
- Characterized all 108 captured Ruby cases before extraction (39 true, 51 false,
  18 null), including path-selected integration requests whose query string is ignored.
- `tests/read_snapshot.rs` exercises actual file/WAL publication and deployment while
  a snapshot stays open, before/after state, write rejection, ownership, drop/error/
  cancellation and no query_only leakage. In-memory tests are not used as writer-
  concurrency evidence.
- `tests/matrix_evaluation.rs` checks all 108 decisions, typed causes/outcomes, null
  candidate semantics, overlap deduplication and rendering after the database closes.
  A pure unit regression makes missing-selector decisions independent of notice prose.
- Before/after release benchmarks: fresh medium (20/5/50) and large (50/10/200)
  datasets, 10 read rounds and 30 seconds mixed load, one run each, sequentially.
  No task builds/tests/reviews run concurrently with a benchmark. Results and final
  validation are recorded with the benchmark comparison before review/push.

## Completion

All 163 Rust tests and every real-client e2e phase pass; formatting and strict Clippy
pass. Fixture bytes, README and routes remain unchanged. All 15 snapshot and 24 evaluator
mutations were killed and restored. The completed independent review found no defects.
The [benchmark comparison](../benchmarks/2026-11-01-matrix-evaluator.md) records all four
release runs: can-i-deploy and matrix p50/p95 are lower at both sizes, with identical
verdict/row counts and zero mixed-load errors. No benchmark-driven optimization followed.
