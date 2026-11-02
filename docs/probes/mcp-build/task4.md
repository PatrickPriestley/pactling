# MCP build task 4: paging and parity

Base: `73deb70`; branch `feat/mcp-task4`.

Before implementation: replace only the two expressly transitional assertions in
`tests/mcp_catalog_transitional.rs`: a multi-page catalog now succeeds with a cursor,
and malformed `previous-process` is CURSOR_INVALID (a syntactically valid absent UUID
is CURSOR_EXPIRED). Existing mixed-field precedence assertions stay unchanged.
All other prior tests and fixture bytes remain protected.

Implementation decision: calculate every page boundary and random UUID token once
when freezing the response. Serialize the complete entry (items, tool, source,
retrievedAt, original pageSize, boundaries and tokens) for the cache budget.
A mutex serializes insertion/lookup, so concurrent replay reads the same immutable
boundaries and tokens. The monotonic creation instant controls five-minute TTL and
insertion order controls eviction; reads never refresh either.

Cancellation evidence (supported by receiver exact source): rmcp 3.8.0
`src/service.rs:1634-1638` removes the request cancellation token and cancels it on
notifications/cancelled. RequestContext.ct is public at 1195-1196. Handler tasks
are independently spawned, so the adapter must explicitly select cancellation
against its HTTP/dispatch future to drop that future and its response resources.
Real stdio cancellation verification is required below.

## Red/green evidence

- `3524af0`: nine initial assertion failures, comprising six real-stdio paging/cap/
  cancellation tests, the two explicitly transitional replacements, and concurrent
  replay. `/tmp/pactling-task4-red.log`. The cancellation failure specifically observed
  an upstream connection remain open after the actual MCP cancellation notification.
- `efe7fbb`: four cache/TTL/error-budget assertions fail against compilation-safe cache
  stubs. `/tmp/pactling-task4-red-limits-final.log`. An earlier atomic-rejection test
  passed immediately against the error-only stub; it was strengthened before its red
  commit to require an existing valid frozen entry that survives a failed insertion.
- `3bcb8ec`: 60 unit tests and eight paging stdio tests pass, including the cancellation
  socket-close assertion. `/tmp/pactling-task4-green-all.log`.
- Output schema validation moved to the common dispatcher success branch, preserving
  the existing guard for both cached and freshly fetched responses.
- Page sizing scans candidates from largest to smallest. It does not assume monotonic
  serialized sizes: the final null cursor is smaller than a continuation token. Each
  candidate includes its actual cursor representation and complete CallToolResult.
- Each page preparation loop yields, outside the cache mutex. Cancellation drops the
  locally owned items/page metadata before insertion. Insertion and cursor lookup are
  synchronized; no partially prepared cache entry is visible.
- Single oversized items anywhere in the frozen response fail the initial call before
  cache insertion. This avoids issuing a cursor that can never finish its listing.
- Cache weight is the serialized full entry collection plus a fixed `size_of::<Instant>`
  charge per entry for monotonic metadata. Serialized tool/source/retrieval time,
  original page size, items, bounds, cursor and next tokens all contribute.

## Parity characterization

The parity tests characterize existing task 3 behavior, not a fabricated implementation
red. `f08aaa1` introduces the three tests; two passed immediately. A dedicated scenario
initially assumed MatrixConsumer version 1 with matrix-regressions was blocked under
canonical cvp. Actual broker evidence showed allowed; matrix-single is blocked and
matches the unchanged captured single_provider_environment scenario. The narrow
`a052424` correction changes that test target and its whole-provider-ignore counterpart,
retains the required blocked assertion, and preserves exact upstream-document equality.
See [parity evidence](task4-parity.md) and `/tmp/pactling-task4-parity-probe.log`.
This is a test scenario correction, not a production bug or claimed TDD red.

## Mutation checks

All 27 initial mutations were killed by executed test failures; no survivors, compile-error kills or timeout kills. Each source file was restored immediately after its individual run. Raw results: `/tmp/pactling-task4-mutations.json`; runner `/tmp/pactling-task4-mutate.py`; individual logs `/tmp/pactling-task4-mutation-<name>.log`.

| Guard or invariant | Initial | Final |
| --- | --- | --- |
| result-cap | killed | killed |
| escaped-text-budget | killed | killed |
| error-size-guard | killed | killed |
| small-error-retains-url | killed | killed |
| oversized-url-removed | killed | killed |
| cache-metadata-budget | killed | killed |
| original-page-size | killed | killed |
| page-boundary-cap | killed | killed |
| continuation-presence | killed | killed |
| page-fit-guard | killed | killed |
| single-item-guard | killed | killed |
| longest-fitting-prefix | killed | killed |
| completion-guard | killed | killed |
| single-entry-budget | killed | killed |
| first-purges-expiry | killed | killed |
| four-entry-cap | killed | killed |
| aggregate-budget | killed | killed |
| oldest-eviction | killed | killed |
| cursor-syntax | killed | killed |
| continuation-purges-expiry | killed | killed |
| cursor-token-binding | killed | killed |
| cursor-tool-binding | killed | killed |
| missing-cursor-expired | killed | killed |
| output-schema-guard | killed | killed |
| cursor-dispatch | killed | killed |
| document-size-guard | killed | killed |
| cancel-inflight | killed | killed |

The output-schema guard was moved, not removed. The previous catalog `items.len() > page_size` guard was intentionally replaced by page construction; its two transitional assertions were replaced before implementation as recorded above. All other original tests/fixtures remain unchanged apart from the approved additive corpus module.

## Additional boundary characterization

`e14a295` adds default 25-item integration pages across 53 broker-ordered items, and a
near-1MiB final page that fits only because its cursor is null. These characterize the
implemented descending candidate search, not additional production changes.
The initial boundary setup measured the 2025 wire result and was slightly too large:
exact rmcp 3.8.0 `model.rs:4951` and `handler/server.rs:258` show that older negotiated
protocols omit `resultType`, while the complete SDK CallToolResult includes it. The
corrected setup measures the complete SDK value, matching the production budget. The
real-stdio test now passes with both items and null, and proves substituting a UUID
would exceed 1MiB. `/tmp/pactling-task4-additional.log`.

Evidence scope: controlled loopback response tests exercise the real production
executable, rmcp stdio protocol, reqwest transport, serialization and cancellation.
Their broker payloads are controlled responses, not additional Ruby compatibility
claims. Corpus parity uses a real pactling broker populated from unchanged committed
fixtures and compares the exact response consumed by MCP via a capturing proxy.

Transitional-test hygiene: the old Session.base field and assignment were removed
because their only use was the superseded oversize assertion; retaining them would
introduce dead-code lint. Other helper behavior is unchanged. The original
`tests/explanation_corpus.rs` content is a byte-identical prefix of the final file;
the MCP module is appended. Existing fixture paths have an empty diff.

## Final verification and handoff

Final source default: **256 tests passed**. Final MCP configuration: **333 tests passed**.
No ignored tests. Both all-target clippy configurations pass with warnings denied;
`cargo fmt --check` passes. The MCP corpus executable passes all four tests, including
its unchanged original all-108 case assertions and exactly three canonical MCP
round trips. The additional selector scenarios compare their entire captured broker
responses, including volatile evaluation fields, without rerunning a verdict to claim
identity. No new matrix evaluator was introduced.

All five final commands exit zero with this environment:

```sh
export CARGO_TARGET_DIR=/tmp/pactling-mcp-probe/target
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export CARGO_PROFILE_DEV_STRIP=symbols CARGO_PROFILE_TEST_STRIP=symbols
export CARGO_INCREMENTAL=0
cargo fmt --check
cargo clippy -j2 --all-targets -- -D warnings
cargo test -j2
cargo clippy -j2 --features mcp --all-targets -- -D warnings
cargo test -j2 --features mcp
```

Raw logs and machine-readable results:

- `/tmp/pactling-task4-fmt-final.log`
- `/tmp/pactling-task4-clippy-default-final.log`
- `/tmp/pactling-task4-test-default-final.log`
- `/tmp/pactling-task4-clippy-mcp-final.log`
- `/tmp/pactling-task4-test-mcp-final.log`
- `/tmp/pactling-task4-verification.json`
- `/tmp/pactling-task4-parity-final.log`
- `/tmp/pactling-task4-additional.log`

Literal excerpts from final MCP output:

```text
test result: ok. 60 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

Task 5 API handoff:

- `McpServer` owns `ReadClient` plus a cloned `Pagination` cache. Its dispatcher selects
  the request context cancellation token against the complete tool future.
- `tools::call(client, cache, request)` normalizes errors and validates every success
  against the discovery output schema, including cached continuations.
- `Pagination::first(tool, items, page_size, route, source).await` prepares and inserts
  a complete frozen entry and returns the success envelope. `next(tool, cursor)` returns
  a saved page envelope or explicit cursor error without an HTTP call.
- `pagination::fits` measures the complete serialized SDK CallToolResult, including
  escaped JSON text and structured content. `error_result` preserves a normal safe
  evidenceUrl and omits the whole optional URL if needed to keep an error under the cap.
- All source fixtures and original schemas stay untouched. No release packaging,
  README/API user documentation, or real agent-client e2e changes are included here.
  Those remain task 5; feature-level non-local review remains required before pushing.

No heavy processes remain running. Generated integration executables were cleaned
only after the final suite completed, using the conductor-provided cleanup script;
no source checkout was removed. No push or independent-review claim is made.
