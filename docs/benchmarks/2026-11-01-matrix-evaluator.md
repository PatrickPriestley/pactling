# Matrix evaluator and read snapshot benchmarks

PR A of #5 / draft PR #28. Baseline binary: main `23b25d6`; after binary: `346243f`.

Release builds on the same Apple M1 Pro / 16 GB macOS 15.6.1 machine as the existing benchmark report. One run per stage and dataset, medium then large, on fresh file-backed WAL databases. Each run used 10 read rounds and 30 seconds of mixed traffic with 8 workers. Task builds, tests, mutations and review processes were stopped during the benchmarks. macOS media-analysis/indexing background activity was visible in process samples in both stages; these single-run numbers are observations, not an isolated statistical study.

## Reads: p50 / p95 milliseconds

| Request | Medium before | Medium after | Large before | Large after |
| --- | --- | --- | --- | --- |
| can-i-deploy consumer to environment | 3.31 / 3.90 | 1.99 / 3.07 | 7.30 / 7.95 | 3.90 / 4.40 |
| can-i-deploy consumer to tag | 3.27 / 3.78 | 1.48 / 1.78 | 5.97 / 6.33 | 2.72 / 3.36 |
| can-i-deploy provider to environment | 12.10 / 13.08 | 5.06 / 6.65 | 36.63 / 39.68 | 17.96 / 20.18 |
| matrix integration (all rows) | 15.71 / 15.98 | 7.12 / 8.57 | 66.39 / 69.47 | 34.41 / 40.43 |

## Mixed traffic

| Dataset/stage | Operations/s | Errors | can-i-deploy p50 / p95 ms |
| --- | --- | --- | --- |
| medium before | 399.7 | 0 | 34.41 / 55.21 |
| medium after | 422.3 | 0 | 6.50 / 15.51 |
| large before | 88.0 | 0 | 166.69 / 239.93 |
| large after | 93.0 | 0 | 12.36 / 70.35 |

Every measured can-i-deploy and matrix read has lower p50 and p95 in the after run; no can-i-deploy slowdown was observed.

The measured read verdicts, row counts and verified-row counts are identical before/after. The shared evaluator and snapshot load distinct version decorations once per decision; the HTTP renderer makes no database calls. No cache, query batching redesign or file-database pool tuning was added.

## Reproduction

Build each revision with `cargo build --release`. Start one broker with a fresh database:

```bash
PACTLING_DATABASE_URL='sqlite:///tmp/matrix-evaluator-medium.db?mode=rwc' PACT_BROKER_PORT=9393 target/release/pactling
```

Run one dataset, stop that broker, then start another with a fresh large database:

```bash
python3 scripts/bench.py http://127.0.0.1:9393 --consumers 20 --providers 5 --versions 50 --rounds 10 --seconds 30 --out medium.json
python3 scripts/bench.py http://127.0.0.1:9393 --consumers 50 --providers 10 --versions 200 --rounds 10 --seconds 30 --out large.json
```

These are the same medium/large dataset dimensions and workload as `2026-11-01.md`. Ten rounds are used for both sizes/stages. Full outputs are retained in the task evidence directory `/tmp/pactling-evaluator-proof/{before,after}-{medium,large}.json`.

## Correctness and safety evidence

- 163 Rust tests pass, including the unchanged 108 Ruby matrix captures and full matrix/environment/deployment document fidelity tests. Fixture bytes are unchanged.
- `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` and `bash e2e/run.sh` pass; all five e2e phases pass, including seven webhook deliveries.
- SQLx 0.9.0 / bundled SQLite 3.51.3 probes prove read-only errors, deferred snapshot isolation and query_only reset on the same pooled connection. File/WAL publication and deployment commit while the reader remains open, with entirely before/after views.
- Snapshot cleanup is tested on completion, drop, errors and cancellation, with raw-pool/migration escapes and transaction upgrades rejected. A memory-only SQLx acquisition setting avoids dropping the sole database connection during cancellation.
- 15 snapshot/ownership/reset mutations and 24 evaluator mutations were killed by behavioral assertions and restored. Typed missing-selector causes are tested independently of notice kind/text. Rendering and evaluation work after the database closes.
- Completed independent review found no defects. The command-line review wrapper failed before returning a review; a completed different-model-family review of the committed diff and exact SQLx source supplied the independent review.
