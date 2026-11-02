# MCP build task 2: HTTP/auth seam

Base: `500c3f3`. Task branch: `feat/mcp-task2`; worktree: `/tmp/pactling-mcp-task2`.
No tool execution/projection implementation, fixture edits, pushes or independent review in this task.
Full real-client e2e and final independent review remain feature-level verification.

## Contract and evidence

- **Verified against the real client/libraries:** reqwest 0.13.5 sends the expected Basic,
  bearer and unauthenticated headers to loopback HTTP receivers. Every fixed route uses GET.
  Percent, slash, question mark, hash and Unicode segments retain their exact encoded request
  targets, including the configured prefix. Redirect responses are returned after one request.
- **Verified against the real client/libraries:** 4 MiB decoded bodies succeed; one additional
  decoded byte fails. The same limit rejects real gzip and zlib/deflate streams, and an unfinished
  chunked body fails at the bound without waiting for EOF. Timeout fires after 30 seconds.
  Upstream semantics in these tests are controlled responders, not additional Ruby fidelity claims.
- **Verified against the real protocol/client:** an h2 0.4.20 peer sends REFUSED_STREAM frames.
  Default reqwest retries made **three** upstream attempts (red); MCP's explicit `retry(never)`
  makes **one** (green). HTTP/2 prior knowledge is test-only; production negotiation is unchanged.
- **Verified against the real executable:** invalid startup identities/base URLs exit before
  initialize, nonzero with empty stdout and fixed, credential-free stderr. Valid Basic/token flags
  initialize without contacting the broker. Existing explain outputs/exits/query fixtures pass.
- **Supported by exact source:** reqwest 0.13.5 `async_impl/client.rs` has feature-dependent
  `Accepts::default`, wraps transport in decompression, and `Response::chunk` reads decoded frames.
  Its tower-http 0.6.11 `decompression/body.rs` uses GzipDecoder/ZlibDecoder. Feature unification
  therefore affects all reqwest clients: explicit opt-out on explain and webhook clients preserves
  their original Accept-Encoding, headers and response handling. Actual compressed receiver tests
  exposed the webhook regression before its fix. No webhook policy or auth behavior changed.
- **Supported by exact source:** url 2.5.8 `path_segments.rs:247` drops exact `.`/`..`; reject
  them and control characters before appending segments. `parser.rs:1035` rejects empty hosts for
  HTTP(S), so the old post-parse host check was unreachable and was removed, recorded in `022b328`.
- Real broker read-only Basic/bearer and encoded-name probes remain the accepted plan's evidence in
  `docs/probes/mcp-adapter/`; no new broker API behavior or fixture values are inferred here.

## Red/green record

1. `b4b0cff`: compile-capable seam placeholders and 14 tests. All 14 fail on absent behavior;
   startup invalid URL stays alive instead of rejecting, configuration accepts invalid values,
   GET returns Unavailable, and typed query pairs are empty. Raw: `/tmp/pactling-task2-red.log`.
   A preliminary shared-target stale-library import failure was discarded; touching `src/lib.rs`
   rebuilt the correct worktree and produced the recorded assertion-red run.
2. `bd29f8d`: isolated red-test hygiene correction after clippy's `unused_io_amount` failure.
   Assert that the synchronization read received bytes instead of discarding the count. This
   strengthens the test; no original behavioral assertion was relaxed. The supervising task
   authorized this narrow correction before the edit.
3. `022b328`: initial implementation green. 14/14 new tests; full default suite 256 passed,
   full MCP suite 275 passed (including two compile-fail boundary doctests); both clippy checks pass.
4. `a5b032d`: record the feature-unification/retry preservation amendment before changing webhook
   production code. Its extra source/test files are explicitly listed in the plan amendment.
5. `c4ec70e`: additional red regression tests and private builder extraction. Actual HTTP/2 test
   fails `left: 3, right: 1`; webhook test fails `MCP feature must not change webhook Accept-Encoding`.
   Four supplemental test assertions kill initial mutation survivors without editing the original
   red tests. Raw: `/tmp/pactling-task2-retry-red.log`, `/tmp/pactling-task2-preservation-red.log`.
6. Fixes pass the unchanged retry test and all five preservation tests. Raw:
   `/tmp/pactling-task2-retry-green.log`, `/tmp/pactling-task2-preservation-green.log`.

## Mutation checks

Every mutation was restored immediately after its targeted test. The final source diff was
inspected to exclude leftover mutations. Initial batch: **24 killed, 4 survived**; supplemental
assertions kill all four survivors. Eight additional checks are killed, giving **36/36 final**.
The table preserves the initial outcomes. The two API-boundary probes fail actual compile-fail
Rust doctests when the private field or a write method is exposed; they do not depend on source
substring checks. Original red tests also contain an older source-string check, retained intact.

Raw logs and JSON summaries: `/tmp/pactling-task2-mutations/` (`summary.json`,
`strengthened-summary.json`, `additional-summary.json`, and one log per named mutation).
Reproduction drivers: `/tmp/pactling-task2-mutate.py`, `/tmp/pactling-task2-strengthen.py`,
`/tmp/pactling-task2-final-mutations.py`.

| Guard/configuration | Initial | Final | Failing test/filter |
| --- | --- | --- | --- |
| `scheme` | killed | killed | `mcp_rejects_url_credentials_and_redirects` |
| `username_url` | killed | killed | `mcp_rejects_url_credentials_and_redirects` |
| `password_url` | killed | killed | `mcp_rejects_url_credentials_and_redirects` |
| `query_url` | killed | killed | `mcp_rejects_url_credentials_and_redirects` |
| `fragment_url` | killed | killed | `mcp_rejects_url_credentials_and_redirects` |
| `dot_path` | killed | killed | `mcp_rejects_dot_and_control_path_segments` |
| `control_path` | killed | killed | `mcp_rejects_dot_and_control_path_segments` |
| `body_cap` | killed | killed | `mcp_bounds_response_reads_and_redacts_errors` |
| `body_cap_boundary` | killed | killed | `mcp_bounds_response_reads_and_redacts_errors` |
| `empty_username` | killed | killed | `mcp_auth_configuration_requires_one_complete_identity` |
| `empty_token` | killed | killed | `mcp_auth_configuration_requires_one_complete_identity` |
| `exclusive_complete_auth` | killed | killed | `mcp_auth_configuration_requires_one_complete_identity` |
| `basic_auth` | killed | killed | `mcp_reuses_basic_token_and_read_only_auth` |
| `token_auth` | killed | killed | `mcp_reuses_basic_token_and_read_only_auth` |
| `latest_tag` | killed | killed | `mcp_typed_explanation_queries_match_recorded_cli_pairs` |
| `ignore_version` | killed | killed | `mcp_typed_explanation_queries_match_recorded_cli_pairs` |
| `redirect` | killed | killed | `mcp_rejects_url_credentials_and_redirects` |
| `timeout_duration` | killed | killed | `mcp_transport_retains_thirty_second_timeout` |
| `timeout_classification` | killed | killed | `mcp_transport_retains_thirty_second_timeout` |
| `matrix_grouping` | survived | killed | `mcp_matrix_query_grouping_and_query_only_names_are_exact` |
| `pact_accept` | survived | killed | `mcp_pact_and_explanation_accept_plain_json_catalogs_accept_hal` |
| `cli_gzip` | survived | killed | `explain_preserves_disabled_response_decoding_with_mcp_enabled` |
| `cli_deflate` | survived | killed | `explain_preserves_disabled_response_decoding_with_mcp_enabled` |
| `cli_empty_tag` | killed | killed | `option_queries_match_recorded_pact_broker_1_77_requests` |
| `cli_version_selector` | killed | killed | `option_queries_match_recorded_pact_broker_1_77_requests` |
| `cli_latest_selector` | killed | killed | `option_queries_match_recorded_pact_broker_1_77_requests` |
| `cli_branch_selector` | killed | killed | `option_queries_match_recorded_pact_broker_1_77_requests` |
| `cli_environment_target` | killed | killed | `option_queries_match_recorded_pact_broker_1_77_requests` |
| `mcp_gzip` | killed | killed | `--test mcp mcp_decoded_body_limit_rejects_compression_bombs -- --exact` |
| `mcp_deflate` | killed | killed | `--test mcp mcp_decoded_body_limit_rejects_compression_bombs -- --exact` |
| `mcp_never_retry` | killed | killed | `--lib broker_client::tests::mcp_does_not_retry_http2_refused_stream -- --exact` |
| `webhook_gzip` | killed | killed | `--test mcp_client_guards webhook_preserves_disabled_response_decoding_with_mcp_enabled -- --exact` |
| `webhook_deflate` | killed | killed | `--test mcp_client_guards webhook_preserves_disabled_response_decoding_with_mcp_enabled -- --exact` |
| `get_method` | killed | killed | `--test mcp mcp_never_accepts_tool_urls_or_write_methods -- --exact` |
| `private_transport` | killed | killed | `--doc` |
| `no_write_interface` | killed | killed | `--doc` |

## Verification commands

All cargo commands use:

```sh
export CARGO_TARGET_DIR=/tmp/pactling-mcp-probe/target
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export CARGO_PROFILE_DEV_STRIP=symbols CARGO_PROFILE_TEST_STRIP=symbols
export CARGO_INCREMENTAL=0
cargo fmt --check
cargo clippy -j2 --all-targets -- -D warnings
cargo test -j2
cargo clippy -j2 --all-targets --features mcp -- -D warnings
cargo test -j2 --features mcp
```

Debug stripping and `-j2` only control disposable local build-cache size/linker peaks; no source
or release-profile changes. Completed generated integration executables may be removed by
`/tmp/pactling-mcp-clean-test-executables.py` between suites; source and the baseline binary are
preserved.

## Handoff API

- `broker_client::ReadClient::new(base, Credentials)` validates immutable startup identity.
  Credentials has optional `username`, `password`, `token` and deliberately has no Debug derive.
- `ReadClient::get(&ReadRoute)` returns `ReadResponse { status, bytes, source_url }` or a safe
  `ClientError::{InvalidConfig, InvalidPath, Timeout, Unavailable, ResponseTooLarge}`. Raw HTTP
  bodies remain bounded bytes for task 3's projection/error mapping; no raw diagnostics are emitted.
- `ReadClient::source_url(&ReadRoute)` uses exactly the same encoded URL construction without I/O.
  Routes are Integrations, Pacticipants, Pact `{consumer, provider, version}`, Matrix
  `{participant_a, participant_b, latestby: Option<Grouping>}`, and Explanation(ExplanationQuery).
  Grouping is Cvpv/Cvp/Cp. There is no arbitrary URL/method/header input or public reqwest handle.
- ExplanationQuery owns pacticipant, selector, target and `Vec<Ignore>`; `.pairs()` is shared with
  `cli::query_pairs`. Selector is Version(String), Latest(Option<String>), Branch(String), MainBranch;
  Target is Environment(String) or Tag(String). Ignore is now in broker_client and re-exported by
  cli to preserve its existing public path. MCP schema validation/length limits remain task 3.
- `McpServer` stores the constructed read client in private `_client`, ready for task 3 dispatch.
  Typed catalog link constructors can be added there using the shared encoded path helper; do
  not expose the underlying client or add arbitrary-URL fetching. Discovery stays task 1's placeholder.

## Final results

All commands exit 0. Default: **256 tests passed**. MCP: **281 tests passed**, including
19 MCP transport/preservation integration tests, the HTTP/2 NACK unit test, and two compile-fail
API-boundary doctests. Counts sum the individual cargo test targets; no test is ignored.
`cargo fmt --check` emits no output. Both all-target clippy runs finish successfully.
No committed red test changes occur in the final green diff.

Literal output excerpts:

```text
MCP clippy:
    Finished `dev` profile [unoptimized] target(s) in 6.26s
Default clippy:
    Finished `dev` profile [unoptimized] target(s) in 5.83s
MCP suite selected target summaries:
test result: ok. 53 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.08s
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.03s
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.04s
test result: ok. 14 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 30.02s
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.07s
test result: ok. 14 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.11s
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.21s
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.39s
```

Complete raw logs:
- `/tmp/pactling-task2-clippy-default-final.log`
- `/tmp/pactling-task2-full-default-final.log`
- `/tmp/pactling-task2-clippy-mcp-final.log`
- `/tmp/pactling-task2-full-mcp-final.log`

Final generated integration-executable cleanup runs only after the completed suites. It preserves
sources, libraries and `/tmp/pactling-mcp-baseline`; no source checkout or fixture is deleted.
