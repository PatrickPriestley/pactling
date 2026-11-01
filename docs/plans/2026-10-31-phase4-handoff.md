# Phase 4 handoff: finish environments, matrix and can-i-deploy

> Done 2026-11-01 on `fix/matrix-verdict-correctness` (PR #18): steps 1 to 5 below are
> complete; the two fidelity tests run, the capture is regenerated with the verifier fix,
> and `e2e/phase4_can_i_deploy.sh` drives the real CLI. Kept for the record.

Written 2026-10-31 for the agent taking over Phase 4. Read `AGENTS.md` first; its rules
apply (fixtures are the evidence, probe before asserting, red before green, mutation-check
guards, independent review before pushing, Conventional Commits with no AI attribution).

## State at handoff (commit `cd5ade9` on `main`)

Phases 1 to 3 are complete, reviewed and green. Phase 4 is about two thirds built:

- Capture is done: `tests/fixtures/ruby-broker/2.121.2/phase4/` (714 files overall) and
  `tests/fixtures/cli/phase4/` hold every environment, deployment, release, matrix and
  can-i-deploy response from Ruby broker 2.121.2, plus the CLI's exact requests as
  `.jsonl`. The verified facts are written up in
  `docs/plans/2026-10-30-pact-broker-implementation.md` under Task 4.1.
- Migration `0004_environments.sql`, `src/db/environments.rs`, `src/db/matrix.rs`,
  `src/service/environments.rs` (with unit tests), `src/http/environments.rs`,
  `src/service/matrix.rs`, `src/http/matrix.rs`, environment links on version documents
  and the deployment-based verification selectors are written and compile.
- `cargo test` passes 109 tests. Two fidelity tests in `tests/matrix.rs` are marked
  `#[ignore]` because the replay does not yet reproduce the capture state and the matrix
  wording still needs iterating against the fixtures.

## What to do, in order

1. **Fix the three matrix bugs Codex found** (GitHub issues #1, #2, #3). They are real and
   sit in `src/service/matrix.rs`: provider-side rows are skipped when there is more than
   one selector; a missing pinned second version falls back to `Target::Any`; the
   `ignore[][version]` check is a placeholder that always matches. Write the failing test
   first for each, then fix, then mutation-check the guard.
2. **Make `cargo clippy --all-targets -- -D warnings` pass** (one type-complexity warning in
   `src/service/matrix.rs`; introduce a `type` alias).
3. **Enable the two ignored fidelity tests.** Diff each failing document against its
   fixture (the tests' `normalise` strips uuids, metadata, verification numbers and
   dates). Expect to adjust both the replay sequence in the test and the wording in
   `src/service/matrix.rs`. Do not weaken fixtures to match the implementation. Known
   capture quirk: in `scripts/capture-ruby-fixtures.sh` the Phase 4 line that runs the
   verifier with `'SELECTORS=[{"branch":"main"}]'` is quoted wrongly and never ran, so the
   fixture `GET_matrix_consumer_2.0.0_test_failed.json` actually records the *unverified*
   state. Fix the line with `env SELECTORS=...`, re-run the capture
   (`bash scripts/capture-ruby-fixtures.sh`, about seven minutes, needs Docker), check the
   recapture with `scripts/compare-fixtures.py`, and commit the regenerated fixtures.
4. **Write `e2e/phase4_can_i_deploy.sh`** following `e2e/phase3_verify.sh`: the real CLI
   creates an environment, records the provider deployment, `can-i-deploy` says yes for
   the consumer, a changed consumer pact is published and the verdict flips to no with the
   captured reason, the provider verifies it (`e2e/provider.test.js`) and the verdict flips
   back to yes. Compare CLI output with `tests/fixtures/cli/phase4/*.txt` where the data
   is deterministic. `e2e/run.sh` picks the script up automatically.
5. Update the Status section of `README.md`, run `agent-review code cd5ade9..HEAD --goal
   "..."` with a reviewer other than your own kind, fix verified findings test-first, then
   push to `origin main`.

Leave issues #4 to #17 alone; they are roadmap items for the maintainer to triage.

## Verifying

```
cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
bash e2e/run.sh
docker compose -p pactling-probe -f scripts/ruby-broker/docker-compose.yml up -d   # reference broker on :9292
```

GitHub Actions does not run on this repository (account billing limit); the commands above
are the CI equivalent.
