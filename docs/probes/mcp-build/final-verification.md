# Final MCP verification

Verified 2026-11-02 UTC after rebasing onto main `26df0eb` (including version 0.1.2).
PR #37 had merged before push. Earlier task records retain the actual pre-rebase commit
IDs used for those probes. The accepted plan's original prototype claims are historical;
production implementation and its limits are recorded here and in task1–task5.

## Final local checks

**Verified against the built production code and real clients:**

- `cargo fmt --check`: passed.
- Default and `--features mcp` `cargo clippy --all-targets -- -D warnings`: passed.
- All default test targets: **256 passed**; all MCP test targets: **333 passed**, including
  two compile-fail boundary doctests. All 108 captured matrix cases and existing fidelity
  tests pass. No fixture bytes or existing explanation schema changed.
- `python3 scripts/test-release.py`: **9 passed**.
- `bash e2e/run.sh`: all five phases passed against both final default and MCP binaries.
- Before/after HTTP probe: **24/24 identical** bodies/status/content types (only host and
  timestamps normalized), covering root, integrations, pacticipants, exact pact, matrix
  and can-i-deploy with HAL, JSON, HTML and wildcard Accept values.

The final aggregate `cargo test` attempt exhausted host disk while creating all test
executables, leaving one incomplete non-executable `read_snapshot` artifact. It is not
claimed as a passing run. After stopping Cargo and removing only generated executables,
all targets were rebuilt/run sequentially, deleting each completed integration executable
before the next. No test was skipped and no assertions were changed. The equivalent commands
were `cargo test --lib --bins`, every `cargo test --test <tests/*.rs stem>`, and `cargo test
--doc`, repeated with `--features mcp`. The corrupt generated artifact was removed and its
rebuild passed. Counts above deduplicate the repeated default library target.

Verification used `-j 2`, `CARGO_TARGET_DIR=/tmp/pactling-mcp-probe/target`,
`CARGO_INCREMENTAL=0`, and debug/test profiles with debug info disabled and symbols stripped.
These resource-saving overrides are not release profile changes. Raw final logs and driver:
`/tmp/pactling-mcp-build-proof/final-{default,mcp}-sequential.log`, `final_checks.py`,
`final-e2e-{default,mcp}.log`, `http-{before,final}.json`.

## Real-client user-mode acceptance

The independent pre-review user-mode run is recorded in
[the real-client evidence](real-client/README.md) and `independent-rerun-summary.json`.
A further run against the final rebased 0.1.2 binary passed:

```sh
PACTLING_BIN=/tmp/pactling-mcp-build-proof/pactling-final-mcp \
  bash e2e/mcp_read_only.sh /tmp/pactling-mcp-final-demo
```

The harness actually runs temporary `codex mcp add` registration, then real Codex 0.166.0
with never-approve policy; no prompt is answered. All five tools are called against demo
broker data. It proves both catalog continuations, exact pact/matrix/explanation documents,
allowed/blocked/unknown, and INVALID_INPUT, NOT_FOUND, RESPONSE_TOO_LARGE, AUTH_REQUIRED,
BROKER_UNAVAILABLE. Five full source documents and four catalog items match. Twelve calls
in the main session plus two separate auth/unavailable sessions return matching structured
and text JSON. Logical database/webhook dumps are unchanged; read-only POST is 401.
[Exact final output](real-client/final-rebased-summary.json) is committed; full rerun frames,
commands and client events remain in `/tmp/pactling-mcp-final-demo`. The earlier complete
sanitized production exchange is committed in `real-client/`.

## Independent review and verified fix

Both complete reviews reported **`Reviewed by: claude sonnet`** and ended with
`<<AGENT-REVIEW-END>>`. Default Claude exhausted usage credits; automatic remote Sonnet
fallback succeeded. No local model was used. Full review: `origin/main..9875f55`;
fix re-review: `9875f55..a349f32`. The reviewer found CI omitted the released MCP feature
set. Exact workflow source confirmed it. A new packaging contract test failed three
assertions before adding MCP clippy/tests and the packaging checks to CI. All nine tests
then passed. Re-review found no critical or important issues remaining in the fix.

| Review-fix guard mutation | Result |
| --- | --- |
| Remove MCP clippy CI command | Killed by workflow contract assertion |
| Remove MCP test CI command | Killed by workflow contract assertion |
| Remove packaging contract CI command | Killed by workflow contract assertion |

Each mutation was restored immediately. Logs: `ci-review-red.log`,
`ci-review-mutations.json`, `review-final.log`, `review-ci-fix.log` in the final proof directory.
Full per-guard build mutation tables, initial survivors and strengthened tests are in
[task 1](task1.md), [task 2](task2.md), [task 3](task3.md), [task 4](task4.md), and
[packaging](task5-packaging.md). No retained guard survivor remains; eleven redundant
new schema type constraints were removed with reachability/redundancy evidence.

Review claims were not treated as independent probe results. In particular, the first
review's incidental assertion that an unknown pacticipant yields 200 unknown is incorrect:
the recorded broker contract is 400 (while a known participant's unknown version can yield
200 blocked). Existing tests/plan preserve the observed behavior. Its Docker feasibility
assessment does not close the missing runtime check.

## Docker/compose acceptance

**Docker/compose runtime proof passed** after the host disk was freed and Docker Desktop
restarted. The `linux/arm64` image built with the default builder. Its MCP adapter ran from its
own container against a broker container and inside the repository's Compose service. Data
persisted across a Compose restart. The probe's leftover container and volume were removed.
Details are in [packaging](task5-packaging.md#dockercompose-verification-after-recovery-2026-11-02).
Native macOS arm64 release/default server and MCP smokes, archive checksum and actual
Homebrew install/test/uninstall passed before the version-only main rebase. The x86_64 Linux
release archive and hosted release Actions are still not proven locally.
