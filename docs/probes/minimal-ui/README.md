# Minimal UI build evidence — 2026-11-01

## Real-system probes

- Maud 0.27.0 with axum 0.8.9 and rustc 1.99.0 compiled in a scratch project;
  HTML escaping and `Html` response content type were asserted before use.
- Saved the pre-UI binary and a freshly seeded file database. Against the same database,
  port and eleven paths, all **88 Accept variants** matched status, content type and body
  before/after the UI build and again after browser form submissions. Only explanation
  `/evaluation/id`, `/evaluation/startedAt` and `/evaluation/evaluatedAt` are normalized.
  See [baseline requests/hashes](accept-baseline.json) and [comparison](accept-comparison.json).
  The baseline capture procedure is `docs/reference/capture-minimal-ui.py`; rechecking must
  reuse the seeded database, not reseed it. Existing fixture bytes are untouched.
- The empty optional version-field probe is [recorded here](form-query-probe.json).
  Branch-head translation uses `latest=true`, consistent with the existing service.
  Unknown pair validation and self-integrations were also checked against the real server.
- System Chrome with JavaScript disabled checked four pages at 1280×900 and 390×844,
  keyboard link focus, local-only requests, and native form submissions for matrix filtering,
  allowed, blocked, unknown and main-branch explanations. Screenshots are saved alongside
  [browser checks](browser-checks.json) and [form checks](browser-form-checks.json).
  A narrow-table layout assertion first failed (49.546875px first column); minimum table
  width fixed it. Horizontal scrolling preserves access to all columns on narrow screens.

To rerun browser checks with a synthetic seeded broker on port 9493, install
`playwright-core` in a scratch directory, set `NODE_PATH` to its `node_modules`, then run
`node docs/probes/minimal-ui/browser-check.cjs OUTPUT_DIRECTORY` and
`node docs/probes/minimal-ui/browser-forms.cjs OUTPUT_DIRECTORY`.
Both scripts use system Chrome on macOS and disable page JavaScript; they do not add a
runtime or project development dependency. These screenshots are inspection artifacts,
not pixel-perfect golden tests. Browser coverage is Chrome, not every browser engine.

## Tests and mutations

After rebase onto main (PR #34 merged), **255 Rust tests pass**, including the 108 captured
matrix cases projected through the shared evaluation into HTML with resolvable evidence
anchors. `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` and
all five phases of `bash e2e/run.sh` pass. Existing read-snapshot concurrency/write-denial/
query_only-reset tests pass; UI tests compare every SQLite table, including a nonempty
pending webhook queue, and prove a subsequent publish still succeeds.

**158 targeted mutations**, all finally killed: task 1 = 9, task 2 = 27, task 3 = 46,
task 4 = 58, task 5 = 18. The [per-guard table](mutation-table.md) records initial and final
results; [normalized data](mutations.json) and each task's ledger accompany it. Twenty-two
initial survivors (two matrix, twenty explanation) gained focused red regression assertions
before reruns killed them. A task-2 disk-full linker failure executed no tests, was not
counted as a survivor or kill, and was retried successfully. All mutations were restored.

Security review is manual plus tests (no repository endpoint checker): routes inherit
existing authentication/read access, there are no UI writes, values are escaped, and links
allow only local paths or HTTP(S). The accepted broker authentication/query contract takes
precedence over generic security-skill defaults. No production API behavior changes;
only the existing explanation validator's visibility widens to `pub(crate)`.

Independent review of record: **completed**, `opencode opencode-go/kimi-k3.1-pro` (OpenCode Go,
Kimi/Moonshot), using the owner's review-fallback tool and `origin/main..HEAD`. The reply
ended with `<<AGENT-REVIEW-END>>` and reported no actionable findings. The reviewer inspected
source and recorded probe evidence; it did not rerun commands under its read-only constraint.
The earlier Cursor attempt and minimal probe both failed with a usage-limit error and did
not count as reviews. Claude was not used; no local model or Gemma review was used.
