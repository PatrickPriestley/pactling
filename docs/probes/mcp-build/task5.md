# MCP build task 5: real client, documentation and release packaging

Base: `459db7a`; worktree `/tmp/pactling-mcp-task5`, branch `feat/mcp-task5`.
The accepted maintainer decision enables MCP in published artifacts while leaving the
source default disabled. Production adapter behavior is unchanged in this task.

## Real-client acceptance

See [commands, literal outputs and evidence](real-client/README.md) and its
[summary](real-client/summary.json). Real Codex CLI 0.166.0 called all five production
tools with read-only Basic credentials against a real file-backed demo broker. Both
catalog continuations, allowed/blocked/unknown explanations, five error classes,
whole-document equality and logical database/webhook-state preservation passed.
No permission/login/trust prompts were answered, no normal MCP config was changed,
and no credentials were copied or read. Fourteen actual results pass their JSON schemas.

`e2e/mcp_read_only.sh` is the account-dependent entry point; `mcp_read_only.py` seeds
through real HTTP routes, records the actual upstream responses, drives the real Codex
CLI, and asserts acceptance. `mcp_frames.py` transparently tees stdio bytes. This is
characterization of existing task 1–4 behavior; no fabricated TDD red is claimed.
The first oversize setup probe exceeded the HTTP publishing input bound; reducing the
metadata to 600 KiB proved the intended MCP output cap against real systems.

User documentation is in `docs/api/mcp.md`, linked as a deliberate Ruby difference in
README. It covers source opt-in versus release inclusion, credential forwarding, all
five contracts, recorded-evidence limits, paging/cache/result caps, errors, standalone
behavior and the reproducible account-dependent command.

## Packaging

The packaging task works independently in `feat/mcp-packaging` from the same base,
then merges into this task before handoff. Its separate report records failing contract
tests, enabling changes, eight killed configuration mutations, native release sizes and
standalone/package checks. No production Rust guards are added or changed by task 5.
Acceptance-harness assertions are test code, not new production validation branches.

## Verification boundary

- Real Codex acceptance: passed; see committed evidence above.
- `bash -n e2e/mcp_read_only.sh`, Python syntax compilation and `git diff --check`: passed.
- Post-run schema validation: fourteen actual outputs passed; four catalog items match
  real source order/fields. Five complete broker documents match without normalization.
- Supervising agent's HTTP fidelity comparison: all 24 baseline/feature responses match
  after the existing host/timestamp normalization, across root, catalogs, exact pact,
  matrix and can-i-deploy and four Accept variants. Logs remain at
  `/tmp/pactling-mcp-build-proof/http-before.json` and `http-after.json`.
- Full feature/default suites, final independent non-local review and final reproducible
  user-mode rerun belong to the supervising agent's task 6. No push/review claim here.

## Independent user-mode rerun

Before final review, the supervising agent independently ran the final documented harness:

```sh
PACTLING_BIN=/tmp/pactling-mcp-build-proof/pactling-mcp-debug \
  bash /tmp/pactling-mcp-task5/e2e/mcp_read_only.sh /tmp/pactling-mcp-owner-demo
```

Exit 0. [Actual summary](real-client/independent-rerun-summary.json): twelve main-session
calls plus one wrong-auth and one unavailable call in separate real Codex sessions;
all five tools, five exact document comparisons, four catalog items, three decisions,
five error classes, read-only POST 401. Logical database/webhook SHA-256 before and after:
`428faa87c7565eb41fd425c702b3dbdab4970b3cc3473c6b3f93824a4a6cf6bd`.
The full rerun output is retained at `/tmp/pactling-mcp-owner-demo`; no model-generated
summary substitutes for the harness assertions. This is additional runtime acceptance,
not the required independent code review.

## Final packaging handoff

Merged `feat/mcp-packaging` at `ce04125`; see the [packaging report](task5-packaging.md).
All eight packaging contract tests pass after merge, and all eight configuration mutations
were killed. Native macOS arm64 release and isolated Homebrew installation/formula tests
pass. MCP adds 6,202,064 bytes (32.35%) to the measured native binary and 52 normal packages;
the 199-package source-default graph is unchanged.

**Task 5's Docker/compose acceptance remains incomplete.** The attempted isolated Docker
builder exhausted scratch disk before compilation, followed by daemon metadata I/O errors.
A normal restart did not recover it. No successful image or container result is claimed.
The packaging report records the two probe resources awaiting cleanup when Docker is
healthy; no owner images/volumes were pruned or reset. No heavy build remains running.
The idle generated Cargo debug/release caches were reclaimed with supervisor authorization;
final task-6 tests will rebuild them. Executable snapshots, all source and evidence remain.
