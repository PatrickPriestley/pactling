# MCP build task 1 — SDK and dispatch

Verified against the real rmcp 3.8.0 SDK and built binary over stdio. Optional dependency
uses only server/macros/transport-io, with default features disabled. Both 2025-06-18 and
2025-11-25 negotiate successfully. Discovery advertises exactly five sorted read tools,
with all four read-only annotations; no resources/prompts capability is advertised.
An invalid server DATABASE_URL and RUST_LOG=trace do not start server configuration or
pollute protocol output. Closing stdin after initialization exits successfully.

This task establishes transport/discovery only. Tool input/output contracts and calls are
implemented in tasks 2–4; discovery's placeholder input schemas are not the final contract.
Broker base URL parsing is only a command option here; auth/URL validation follows in task 2.
Request cancellation/resource release with an in-flight broker call belongs to task 4.

## Red and green

- Default `cargo test --test mcp_dispatch`: assertion failed because the old binary said
  "unexpected argument 'mcp'", not the required feature-disabled diagnostic.
- Feature-enabled `cargo test --features mcp --test mcp_dispatch`: all three tests failed
  on missing initialize replies, before implementation.
- `cargo test --features mcp --test mcp_help`: assertion failed because Clap wrote help
  on stdout. Help now goes to stderr, exits successfully, and invalid options produce a
  fixed diagnostic without echoing user values.
- Default full suite: 256 passed. Final feature-enabled full suite: 259 passed.
- Existing tests and Ruby fixture bytes were untouched. The two red-test commits were not
  edited during green implementation. `cargo fmt --check` and
  `cargo clippy --all-targets --features mcp -- -D warnings` pass.

Build commands used CARGO_TARGET_DIR=/tmp/pactling-mcp-probe/target,
CARGO_PROFILE_DEV_DEBUG=0, CARGO_PROFILE_TEST_DEBUG=0 and CARGO_INCREMENTAL=0 to fit scratch
disk space; these do not alter committed release settings. Baseline binary was preserved
before green at /tmp/pactling-mcp-baseline for later HTTP/server comparison.

## Mutations

Each mutation was restored immediately; each failed a behavioral assertion, not compilation.

| Guard/behavior | Mutation | Test | Result |
| --- | --- | --- | --- |
| MCP early dispatch | Match a different command name | mcp_dispatch | killed |
| MCP return prevents server fallthrough | Continue into server after MCP session | mcp_dispatch | killed |
| Help-versus-error branch | Invert Clap error-use-stderr predicate | mcp_help | killed |
| Invalid option exit | Return success for invalid options | mcp_help | killed |
| Protocol-only stdout | Print help on stdout | mcp_help | killed |
| Read-only tool hint | Advertise readOnlyHint=false | mcp_dispatch | killed |

Raw local logs: /tmp/mcp-task1-red-default.log, /tmp/mcp-task1-red-enabled.log,
/tmp/mcp-task1-help-red.log, /tmp/mcp-task1-suite-default.log,
/tmp/mcp-task1-suite-final.log and /tmp/mcp-task1-mutant-*.log.
