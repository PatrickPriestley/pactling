# MCP build task 3: tool contracts and projections

Base: `1b453e5`. Branch `feat/mcp-task3`; worktree `/tmp/pactling-mcp-task3`.
No push, existing fixture edits, new broker routes, matrix evaluation rules, writes or
independent review. Final real-client e2e/release/review belongs to task 5/6.

## Evidence and decisions

- **Verified against the real SDK/client libraries:** rmcp 3.8.0 stdio initialization,
  discovery and `tools/call` run through a spawned production executable. Actual reqwest
  GETs reach loopback HTTP responders; no broker href is followed. These controlled
  response tests establish adapter behavior, not new Ruby compatibility evidence.
- **Existing real-system evidence:** broker response shapes and query semantics come from
  committed Ruby fixtures, explanation fixtures and `docs/probes/mcp-adapter/` captures.
  Tests replay missing-selector captures and preserve the broker's blocked decision.
  Full CLI-equivalent query targets cover five selector forms, two target forms and both
  ignore forms. Task 2's shared query builder is reused unchanged.
- **Verified against the real validator:** jsonschema 0.60.2 compiles a bundled explanation
  schema retaining its own `$id` and local `#/$defs` references; a deeply invalid source
  kind fails validation. Raw probe: `/tmp/pactling-task3-jsonschema-probe.log`.
  `jsonschema` becomes an optional runtime dependency only with feature `mcp`, using the
  same version/default-feature-disabled configuration already present for tests. Runtime
  recursively validates the exact discovery schemas, avoiding a divergent hand validator.
- **Supported by exact SDK source and exercised over stdio:** manually implementing
  `ServerHandler::call_tool` permits safe structured known-tool validation errors. The SDK
  `CallToolResult::structured`/`structured_error` serialize identical JSON text and
  structured content. Unknown tools remain protocol errors. No second verdict is created.
- Explanation documents remain unchanged. The original v1 schema and fixture bytes remain
  untouched. The accepted unsupported-major contract required a narrow derived bundle:
  only decision/error `schemaVersion` constraints change from literal `1.0` to numeric
  v1 pattern `^1\.[0-9]+$`. All remaining schema content is equality-tested after normalizing
  those two nodes; local references still resolve in the embedded resource. `1.9` is
  preserved unchanged; `2.0` and malformed versions fail. The plan records this clarification.
- Catalog link constructors accept only typed fixed paths. Required links for dot/control
  names cannot be represented safely; a whole-call fixed error with safe catalog source URL
  reports the adapter limitation. Query-only matrix/explanation names remain accepted.
  Foreign origins, encoded Unicode names/prefixes and mutation relations are covered.

## Red/green record

1. `d3b850e`: 20 initial tests; **19 assertion failures**, one existing annotation
   characterization already green. Missing dispatch returned protocol method-not-found;
   discovery lacked output schemas and complete untrusted-data descriptions. Raw:
   `/tmp/pactling-task3-red.log`.
2. `dfab7a3`: catalog path clarification plus supplemental mixed valid/dot/control test.
   Recorded assertion red: `/tmp/pactling-task3-catalog-red.log`. The nested schema test
   served as the library probe, not a claimed missing-behavior red.
3. `725c87e`: authorized lint-only removal of three unused duplicated helper members from
   the supplemental test; no assertion changed. Original `tests/mcp_tools.rs` stays byte-identical.
4. `335c6c5`: dispatch/projections/schemas green, 22 tests pass. Raw:
   `/tmp/pactling-task3-green.log`; all-target MCP clippy passes in
   `/tmp/pactling-task3-clippy-initial.log`.
5. `4e6a780`: separate guard tests and plan amendment. JSON `pageSize:1.0` incorrectly used
   default 25, and a valid `schemaVersion:1.9` was rejected. Both fail assertions before fixes:
   `/tmp/pactling-task3-boundary-red.log`, `/tmp/pactling-task3-minor-red.log` and
   `/tmp/pactling-task3-amendment-red.log`. The supervising agent explicitly authorized
   replacing the supplemental resource-equivalence assertion only as described above.
6. `000167c`: numeric page-size extraction and narrowly derived v1 constraints fix both
   failures; v1 error classification reuses that same version constraint. Raw green:
   `/tmp/pactling-task3-boundary-green.log`.

## Task 4 seam and remaining work

`McpServer` owns the immutable `ReadClient`. `tools::call` handles known-tool lookup,
explicit validation and structured SDK wrapping. `execute` builds the fixed `ReadRoute`,
fetches once, maps errors and projects catalogs; `schema::success` creates the envelope.
`project_catalog` returns complete projected items and `complete_catalog` is the temporary
paging seam. Replace this with process-local frozen-response state; persist retrievedAt
and page size there. The caller currently has no cancellation context; thread the SDK
context/token through `tools::call` for task 4. `ToolError::too_large` provides route-aware
guidance and source URL, ready for final serialized-result bounding.

**Transitional only:** catalogs larger than requested/default page size explicitly fail
rather than truncate; cursor-only calls return CURSOR_EXPIRED without HTTP. These behaviors
are isolated in `tests/mcp_catalog_transitional.rs` for intentional task-4 replacement.
The full 1 MiB SDK-result cap, cache bounds/eviction/replay/expiry, malformed/cross-tool cursor
classification, cancellation and corpus parity are task 4. A final error can itself exceed
1 MiB because the encoded query URL can be large: task 4 must bound the whole serialized
SDK result and may omit optional evidenceUrl in the fixed fallback. Do not truncate data
or silently alter decision documents.

## Mutation and final verification

All mutated sources/schema files were restored immediately after each run. Initial
source checks: 22 killed, 13 survived, one compile-error mutation harness and one missing
replacement pattern. Strengthened witnesses kill the 13 survivors; corrected harnesses
kill the other two: **37/37 source checks killed**. Compile failures are not counted as kills.
Two additional compiled mutations of the derived decision/error version patterns are killed.

Schema checks use a prebuilt integration test executable loading each checked-in schema
file through the actual jsonschema 0.60.2 validator. The driver changes exactly one
constraint, runs the assertion suite and restores the original bytes. Runtime dispatch's
input/output validator wiring is separately mutation-checked in the compiled production
module, and discovery/file identity is tested. This avoids 378 needless recompilations;
it does not substitute a permissive fake validator. Initial **257/378 killed**, with 121
survivors. A separate negative-witness suite kills 110 survivors. The remaining eleven
constraints are deleted with proof in `e1a5191`: ten branch object checks cannot reject
anything beyond the enclosing object root; the explanation's adjacent allOf object check
cannot reject anything beyond the object-only embedded v1 resource. They are newly authored,
not inherited v1 definitions. **367 retained schema constraints killed; 11 redundant removed.**
Seven root object constraints are also mandated MCP schema metadata, so their mutation is
caught by the advertised-root-type contract assertion even where another validation rule
already excludes nonobjects. No inherited v1 definition is claimed as newly mutation-tested.

Raw source records: `/tmp/pactling-task3-mutations/`, `/tmp/pactling-task3-strengthened/`,
`/tmp/pactling-task3-version-mutations/`. Raw schema records:
`/tmp/pactling-task3-schema-mutations/`, `/tmp/pactling-task3-schema-strengthened/`.
Each has `summary.json` and per-mutation assertion logs. Drivers are
`/tmp/pactling-task3-mutate.py`, `/tmp/pactling-task3-strengthen.py`,
`/tmp/pactling-task3-version-mutate.py`, `/tmp/pactling-task3-schema-mutate.py`, and
`/tmp/pactling-task3-schema-strengthen.py`.

Transitional cursor/page guards are marked below; task 4 intentionally replaces their
behavior, preserving validation precedence. Empty cursors remain schema-invalid
INVALID_INPUT. CURSOR_INVALID is for nonempty schema-valid malformed/cross-tool tokens;
missing/expired state is CURSOR_EXPIRED. Simultaneous cursor/pageSize still wins with
SCHEMA_VALIDATION and `/cursor`, even when field types are wrong.

### Source guards

| Guard | Initial | Final | Failing assertion test |
| --- | --- | --- | --- |

| `input_validation` | killed | killed | `mcp_rejects_extra_main_branch_fields` |
| `cursor_precedence` | survived | killed | `mcp_guards_cursor_precedence_and_temporary_expiry` |
| `cursor_expiry (transitional)` | survived | killed | `mcp_guards_cursor_precedence_and_temporary_expiry` |
| `http_status` | killed | killed | `mcp_missing_explanation_route_is_capability_error` |
| `output_shape` | killed | killed | `mcp_malformed_responses_and_contradictory_verdicts` |
| `catalog_page_bound (transitional)` | survived | killed | `mcp_transitional_catalog_does_not_truncate` |
| `catalog_relation_absent` | survived | killed | `mcp_guards_catalog_relation_presence_and_complete_pages` |
| `catalog_relation_present` | killed | killed | `mcp_catalog_links_use_configured_origin` |
| `latest_stub` | killed | killed | `mcp_pacticipant_latest_version_absent_and_malformed` |
| `missing_collection` | compile_error | killed | `mcp_malformed_responses_and_contradictory_verdicts` |
| `required_catalog_string` | killed | killed | `mcp_malformed_responses_and_contradictory_verdicts` |
| `unknown_tool` | survived | killed | `mcp_guards_transport_path_and_unknown_tool_errors` |
| `notfound_prefix` | survived | killed | `mcp_guards_reject_false_not_found_and_environment_matches` |
| `notfound_suffix` | survived | killed | `mcp_guards_reject_false_not_found_and_environment_matches` |
| `notfound_plain` | killed | killed | `mcp_error_codes_are_stable` |
| `env_schema` | survived | killed | `mcp_guards_reject_false_not_found_and_environment_matches` |
| `env_code` | survived | killed | `mcp_guards_reject_false_not_found_and_environment_matches` |
| `env_details` | pattern_missing | killed | `mcp_unknown_environment_detail_fallback` |
| `capability_404` | killed | killed | `mcp_missing_explanation_route_is_capability_error` |
| `recognized_404` | killed | killed | `mcp_error_codes_are_stable` |
| `status_401` | killed | killed | `mcp_error_codes_are_stable` |
| `status_403` | killed | killed | `mcp_error_codes_are_stable` |
| `status_429` | killed | killed | `mcp_error_codes_are_stable` |
| `status_500to599` | killed | killed | `mcp_error_codes_are_stable` |
| `status_300to399` | killed | killed | `mcp_error_codes_are_stable` |
| `status_414_431` | killed | killed | `mcp_error_codes_are_stable` |
| `status_400_422` | killed | killed | `mcp_error_codes_are_stable` |
| `timeout_mapping` | survived | killed | `mcp::error_tests::transport_error_mapping_is_fixed_and_safe` |
| `unavailable_mapping` | survived | killed | `mcp::error_tests::transport_error_mapping_is_fixed_and_safe` |
| `invalidpath_mapping` | survived | killed | `mcp::error_tests::transport_error_mapping_is_fixed_and_safe` |
| `upstream_bound_mapping` | killed | killed | `mcp_oversize_guidance_depends_on_grouping` |
| `matrix_guidance_grouping` | killed | killed | `mcp_oversize_guidance_depends_on_grouping` |
| `matrix_guidance_nongrouping` | killed | killed | `mcp_oversize_guidance_depends_on_grouping` |
| `catalog_path_rejection` | killed | killed | `mcp_catalog_unrepresentable_names_fail_without_partial_data` |
| `catalog_matrix_path` | killed | killed | `mcp_catalog_links_use_configured_origin` |
| `catalog_dashboard_path` | survived | killed | `mcp_guards_catalog_relation_presence_and_complete_pages` |
| `catalog_participant_path` | killed | killed | `mcp_catalog_links_use_configured_origin` |
| `decision_numeric_v1_pattern` | killed | killed | `mcp_preserves_numeric_v1_minor_documents_and_rejects_other_versions` |
| `error_numeric_v1_pattern` | killed | killed | `mcp_guards_reject_false_not_found_and_environment_matches` |

The remaining `unreachable!` match arms are exhaustiveness assertions after schema-enforced
closed string unions or selection of one of five static contracts. No valid known-tool input
can enter them. No guard is weakened to preserve a silent fallback.

### Schema constraints

`input` rows use `mcp_input_schema_guards_match_contract`; `output` rows use
`mcp_output_schema_guards_match_contract`. Initial survivors use
`mcp_schema_survivors_have_invalid_witnesses` in the strengthened run. Required-array
indices refer to each individual required field in the checked-in schema; every row's
raw record includes the full file/path/index and log filename. Boundary positives include
1024-character names, 100 ignores, page sizes 1/100/1.0, nullable latest versions,
all selector/target variants, and empty catalogs.

| Schema constraint | Initial | Final |
| --- | --- | --- |
| `pactling_explain_deployment.input:type` | killed | killed |
| `pactling_explain_deployment.input:properties/pacticipant/type` | killed | killed |
| `pactling_explain_deployment.input:properties/pacticipant/minLength` | killed | killed |
| `pactling_explain_deployment.input:properties/pacticipant/maxLength` | killed | killed |
| `pactling_explain_deployment.input:properties/selector/oneOf` | killed | killed |
| `pactling_explain_deployment.input:properties/selector/oneOf/0/type` | survived | killed |
| `pactling_explain_deployment.input:properties/selector/oneOf/0/properties/kind/const` | killed | killed |
| `pactling_explain_deployment.input:properties/selector/oneOf/0/properties/version/type` | killed | killed |
| `pactling_explain_deployment.input:properties/selector/oneOf/0/properties/version/minLength` | killed | killed |
| `pactling_explain_deployment.input:properties/selector/oneOf/0/properties/version/maxLength` | killed | killed |
| `pactling_explain_deployment.input:properties/selector/oneOf/0/required/0` | killed | killed |
| `pactling_explain_deployment.input:properties/selector/oneOf/0/required/1` | killed | killed |
| `pactling_explain_deployment.input:properties/selector/oneOf/0/additionalProperties` | killed | killed |
| `pactling_explain_deployment.input:properties/selector/oneOf/1/type` | survived | killed |
| `pactling_explain_deployment.input:properties/selector/oneOf/1/properties/kind/const` | killed | killed |
| `pactling_explain_deployment.input:properties/selector/oneOf/1/properties/tag/type` | killed | killed |
| `pactling_explain_deployment.input:properties/selector/oneOf/1/properties/tag/minLength` | killed | killed |
| `pactling_explain_deployment.input:properties/selector/oneOf/1/properties/tag/maxLength` | killed | killed |
| `pactling_explain_deployment.input:properties/selector/oneOf/1/required/0` | killed | killed |
| `pactling_explain_deployment.input:properties/selector/oneOf/1/additionalProperties` | killed | killed |
| `pactling_explain_deployment.input:properties/selector/oneOf/2/type` | survived | killed |
| `pactling_explain_deployment.input:properties/selector/oneOf/2/properties/kind/const` | killed | killed |
| `pactling_explain_deployment.input:properties/selector/oneOf/2/properties/branch/type` | killed | killed |
| `pactling_explain_deployment.input:properties/selector/oneOf/2/properties/branch/minLength` | killed | killed |
| `pactling_explain_deployment.input:properties/selector/oneOf/2/properties/branch/maxLength` | killed | killed |
| `pactling_explain_deployment.input:properties/selector/oneOf/2/required/0` | killed | killed |
| `pactling_explain_deployment.input:properties/selector/oneOf/2/required/1` | killed | killed |
| `pactling_explain_deployment.input:properties/selector/oneOf/2/additionalProperties` | killed | killed |
| `pactling_explain_deployment.input:properties/selector/oneOf/3/type` | survived | killed |
| `pactling_explain_deployment.input:properties/selector/oneOf/3/properties/kind/const` | killed | killed |
| `pactling_explain_deployment.input:properties/selector/oneOf/3/required/0` | killed | killed |
| `pactling_explain_deployment.input:properties/selector/oneOf/3/additionalProperties` | killed | killed |
| `pactling_explain_deployment.input:properties/target/oneOf` | killed | killed |
| `pactling_explain_deployment.input:properties/target/oneOf/0/type` | survived | killed |
| `pactling_explain_deployment.input:properties/target/oneOf/0/properties/kind/const` | killed | killed |
| `pactling_explain_deployment.input:properties/target/oneOf/0/properties/name/type` | killed | killed |
| `pactling_explain_deployment.input:properties/target/oneOf/0/properties/name/minLength` | killed | killed |
| `pactling_explain_deployment.input:properties/target/oneOf/0/properties/name/maxLength` | killed | killed |
| `pactling_explain_deployment.input:properties/target/oneOf/0/required/0` | killed | killed |
| `pactling_explain_deployment.input:properties/target/oneOf/0/required/1` | killed | killed |
| `pactling_explain_deployment.input:properties/target/oneOf/0/additionalProperties` | killed | killed |
| `pactling_explain_deployment.input:properties/target/oneOf/1/type` | survived | killed |
| `pactling_explain_deployment.input:properties/target/oneOf/1/properties/kind/const` | killed | killed |
| `pactling_explain_deployment.input:properties/target/oneOf/1/properties/name/type` | killed | killed |
| `pactling_explain_deployment.input:properties/target/oneOf/1/properties/name/minLength` | killed | killed |
| `pactling_explain_deployment.input:properties/target/oneOf/1/properties/name/maxLength` | killed | killed |
| `pactling_explain_deployment.input:properties/target/oneOf/1/required/0` | killed | killed |
| `pactling_explain_deployment.input:properties/target/oneOf/1/required/1` | killed | killed |
| `pactling_explain_deployment.input:properties/target/oneOf/1/additionalProperties` | killed | killed |
| `pactling_explain_deployment.input:properties/ignores/type` | killed | killed |
| `pactling_explain_deployment.input:properties/ignores/maxItems` | killed | killed |
| `pactling_explain_deployment.input:properties/ignores/items/type` | survived | killed |
| `pactling_explain_deployment.input:properties/ignores/items/properties/pacticipant/type` | killed | killed |
| `pactling_explain_deployment.input:properties/ignores/items/properties/pacticipant/minLength` | killed | killed |
| `pactling_explain_deployment.input:properties/ignores/items/properties/pacticipant/maxLength` | killed | killed |
| `pactling_explain_deployment.input:properties/ignores/items/properties/version/type` | killed | killed |
| `pactling_explain_deployment.input:properties/ignores/items/properties/version/minLength` | killed | killed |
| `pactling_explain_deployment.input:properties/ignores/items/properties/version/maxLength` | killed | killed |
| `pactling_explain_deployment.input:properties/ignores/items/required/0` | killed | killed |
| `pactling_explain_deployment.input:properties/ignores/items/additionalProperties` | killed | killed |
| `pactling_explain_deployment.input:required/0` | killed | killed |
| `pactling_explain_deployment.input:required/1` | killed | killed |
| `pactling_explain_deployment.input:required/2` | killed | killed |
| `pactling_explain_deployment.input:additionalProperties` | killed | killed |
| `pactling_explain_deployment.output:type` | survived | killed |
| `pactling_explain_deployment.output:oneOf` | killed | killed |
| `pactling_explain_deployment.output:oneOf/0/type` | survived | removed: redundant |
| `pactling_explain_deployment.output:oneOf/0/properties/adapterSchemaVersion/const` | killed | killed |
| `pactling_explain_deployment.output:oneOf/0/properties/data/allOf/1/type` | survived | removed: redundant |
| `pactling_explain_deployment.output:oneOf/0/properties/data/allOf/1/required/0` | survived | killed |
| `pactling_explain_deployment.output:oneOf/0/properties/coverage/const` | killed | killed |
| `pactling_explain_deployment.output:oneOf/0/properties/evidenceLinks/type` | killed | killed |
| `pactling_explain_deployment.output:oneOf/0/properties/evidenceLinks/items/type` | survived | killed |
| `pactling_explain_deployment.output:oneOf/0/properties/evidenceLinks/items/properties/rel/type` | killed | killed |
| `pactling_explain_deployment.output:oneOf/0/properties/evidenceLinks/items/properties/href/type` | killed | killed |
| `pactling_explain_deployment.output:oneOf/0/properties/evidenceLinks/items/required/0` | killed | killed |
| `pactling_explain_deployment.output:oneOf/0/properties/evidenceLinks/items/required/1` | killed | killed |
| `pactling_explain_deployment.output:oneOf/0/properties/evidenceLinks/items/additionalProperties` | killed | killed |
| `pactling_explain_deployment.output:oneOf/0/required/0` | killed | killed |
| `pactling_explain_deployment.output:oneOf/0/required/1` | killed | killed |
| `pactling_explain_deployment.output:oneOf/0/required/2` | killed | killed |
| `pactling_explain_deployment.output:oneOf/0/required/3` | killed | killed |
| `pactling_explain_deployment.output:oneOf/0/additionalProperties` | killed | killed |
| `pactling_explain_deployment.output:oneOf/1/type` | survived | removed: redundant |
| `pactling_explain_deployment.output:oneOf/1/properties/adapterSchemaVersion/const` | survived | killed |
| `pactling_explain_deployment.output:oneOf/1/properties/error/type` | survived | killed |
| `pactling_explain_deployment.output:oneOf/1/properties/error/properties/code/type` | survived | killed |
| `pactling_explain_deployment.output:oneOf/1/properties/error/properties/message/type` | survived | killed |
| `pactling_explain_deployment.output:oneOf/1/properties/error/properties/retryable/type` | survived | killed |
| `pactling_explain_deployment.output:oneOf/1/properties/error/properties/httpStatus/type` | survived | killed |
| `pactling_explain_deployment.output:oneOf/1/properties/error/properties/evidenceUrl/type` | survived | killed |
| `pactling_explain_deployment.output:oneOf/1/properties/error/properties/details/type` | survived | killed |
| `pactling_explain_deployment.output:oneOf/1/properties/error/properties/details/properties/reasonCode/type` | survived | killed |
| `pactling_explain_deployment.output:oneOf/1/properties/error/properties/details/properties/field/type` | survived | killed |
| `pactling_explain_deployment.output:oneOf/1/properties/error/properties/details/required/0` | survived | killed |
| `pactling_explain_deployment.output:oneOf/1/properties/error/properties/details/additionalProperties` | survived | killed |
| `pactling_explain_deployment.output:oneOf/1/properties/error/required/0` | killed | killed |
| `pactling_explain_deployment.output:oneOf/1/properties/error/required/1` | killed | killed |
| `pactling_explain_deployment.output:oneOf/1/properties/error/required/2` | killed | killed |
| `pactling_explain_deployment.output:oneOf/1/properties/error/additionalProperties` | survived | killed |
| `pactling_explain_deployment.output:oneOf/1/required/0` | survived | killed |
| `pactling_explain_deployment.output:oneOf/1/required/1` | survived | killed |
| `pactling_explain_deployment.output:oneOf/1/additionalProperties` | survived | killed |
| `pactling_get_pact.input:type` | killed | killed |
| `pactling_get_pact.input:properties/consumer/type` | killed | killed |
| `pactling_get_pact.input:properties/consumer/minLength` | killed | killed |
| `pactling_get_pact.input:properties/consumer/maxLength` | killed | killed |
| `pactling_get_pact.input:properties/provider/type` | killed | killed |
| `pactling_get_pact.input:properties/provider/minLength` | killed | killed |
| `pactling_get_pact.input:properties/provider/maxLength` | killed | killed |
| `pactling_get_pact.input:properties/consumerVersion/type` | killed | killed |
| `pactling_get_pact.input:properties/consumerVersion/minLength` | killed | killed |
| `pactling_get_pact.input:properties/consumerVersion/maxLength` | killed | killed |
| `pactling_get_pact.input:required/0` | killed | killed |
| `pactling_get_pact.input:required/1` | killed | killed |
| `pactling_get_pact.input:required/2` | killed | killed |
| `pactling_get_pact.input:additionalProperties` | killed | killed |
| `pactling_get_pact.output:type` | survived | killed |
| `pactling_get_pact.output:oneOf` | killed | killed |
| `pactling_get_pact.output:oneOf/0/type` | survived | removed: redundant |
| `pactling_get_pact.output:oneOf/0/properties/adapterSchemaVersion/const` | killed | killed |
| `pactling_get_pact.output:oneOf/0/properties/data/type` | killed | killed |
| `pactling_get_pact.output:oneOf/0/properties/coverage/const` | killed | killed |
| `pactling_get_pact.output:oneOf/0/properties/evidenceLinks/type` | killed | killed |
| `pactling_get_pact.output:oneOf/0/properties/evidenceLinks/items/type` | survived | killed |
| `pactling_get_pact.output:oneOf/0/properties/evidenceLinks/items/properties/rel/type` | killed | killed |
| `pactling_get_pact.output:oneOf/0/properties/evidenceLinks/items/properties/href/type` | killed | killed |
| `pactling_get_pact.output:oneOf/0/properties/evidenceLinks/items/required/0` | killed | killed |
| `pactling_get_pact.output:oneOf/0/properties/evidenceLinks/items/required/1` | killed | killed |
| `pactling_get_pact.output:oneOf/0/properties/evidenceLinks/items/additionalProperties` | killed | killed |
| `pactling_get_pact.output:oneOf/0/required/0` | killed | killed |
| `pactling_get_pact.output:oneOf/0/required/1` | killed | killed |
| `pactling_get_pact.output:oneOf/0/required/2` | killed | killed |
| `pactling_get_pact.output:oneOf/0/required/3` | killed | killed |
| `pactling_get_pact.output:oneOf/0/additionalProperties` | killed | killed |
| `pactling_get_pact.output:oneOf/1/type` | survived | removed: redundant |
| `pactling_get_pact.output:oneOf/1/properties/adapterSchemaVersion/const` | survived | killed |
| `pactling_get_pact.output:oneOf/1/properties/error/type` | survived | killed |
| `pactling_get_pact.output:oneOf/1/properties/error/properties/code/type` | survived | killed |
| `pactling_get_pact.output:oneOf/1/properties/error/properties/message/type` | survived | killed |
| `pactling_get_pact.output:oneOf/1/properties/error/properties/retryable/type` | survived | killed |
| `pactling_get_pact.output:oneOf/1/properties/error/properties/httpStatus/type` | survived | killed |
| `pactling_get_pact.output:oneOf/1/properties/error/properties/evidenceUrl/type` | survived | killed |
| `pactling_get_pact.output:oneOf/1/properties/error/properties/details/type` | survived | killed |
| `pactling_get_pact.output:oneOf/1/properties/error/properties/details/properties/reasonCode/type` | survived | killed |
| `pactling_get_pact.output:oneOf/1/properties/error/properties/details/properties/field/type` | survived | killed |
| `pactling_get_pact.output:oneOf/1/properties/error/properties/details/required/0` | survived | killed |
| `pactling_get_pact.output:oneOf/1/properties/error/properties/details/additionalProperties` | survived | killed |
| `pactling_get_pact.output:oneOf/1/properties/error/required/0` | killed | killed |
| `pactling_get_pact.output:oneOf/1/properties/error/required/1` | killed | killed |
| `pactling_get_pact.output:oneOf/1/properties/error/required/2` | killed | killed |
| `pactling_get_pact.output:oneOf/1/properties/error/additionalProperties` | survived | killed |
| `pactling_get_pact.output:oneOf/1/required/0` | survived | killed |
| `pactling_get_pact.output:oneOf/1/required/1` | survived | killed |
| `pactling_get_pact.output:oneOf/1/additionalProperties` | survived | killed |
| `pactling_list_integrations.input:type` | survived | killed |
| `pactling_list_integrations.input:properties/pageSize/type` | killed | killed |
| `pactling_list_integrations.input:properties/pageSize/minimum` | killed | killed |
| `pactling_list_integrations.input:properties/pageSize/maximum` | killed | killed |
| `pactling_list_integrations.input:properties/cursor/type` | killed | killed |
| `pactling_list_integrations.input:properties/cursor/minLength` | killed | killed |
| `pactling_list_integrations.input:additionalProperties` | killed | killed |
| `pactling_list_integrations.input:not` | killed | killed |
| `pactling_list_integrations.input:not/required/0` | killed | killed |
| `pactling_list_integrations.input:not/required/1` | killed | killed |
| `pactling_list_integrations.output:type` | survived | killed |
| `pactling_list_integrations.output:oneOf` | killed | killed |
| `pactling_list_integrations.output:oneOf/0/type` | survived | removed: redundant |
| `pactling_list_integrations.output:oneOf/0/properties/adapterSchemaVersion/const` | killed | killed |
| `pactling_list_integrations.output:oneOf/0/properties/data/type` | killed | killed |
| `pactling_list_integrations.output:oneOf/0/properties/data/properties/items/type` | killed | killed |
| `pactling_list_integrations.output:oneOf/0/properties/data/properties/items/items/type` | survived | killed |
| `pactling_list_integrations.output:oneOf/0/properties/data/properties/items/items/properties/consumer/type` | killed | killed |
| `pactling_list_integrations.output:oneOf/0/properties/data/properties/items/items/properties/provider/type` | killed | killed |
| `pactling_list_integrations.output:oneOf/0/properties/data/properties/items/items/properties/evidenceLinks/type` | killed | killed |
| `pactling_list_integrations.output:oneOf/0/properties/data/properties/items/items/properties/evidenceLinks/items/type` | survived | killed |
| `pactling_list_integrations.output:oneOf/0/properties/data/properties/items/items/properties/evidenceLinks/items/properties/rel/type` | survived | killed |
| `pactling_list_integrations.output:oneOf/0/properties/data/properties/items/items/properties/evidenceLinks/items/properties/href/type` | survived | killed |
| `pactling_list_integrations.output:oneOf/0/properties/data/properties/items/items/properties/evidenceLinks/items/required/0` | survived | killed |
| `pactling_list_integrations.output:oneOf/0/properties/data/properties/items/items/properties/evidenceLinks/items/required/1` | survived | killed |
| `pactling_list_integrations.output:oneOf/0/properties/data/properties/items/items/properties/evidenceLinks/items/additionalProperties` | survived | killed |
| `pactling_list_integrations.output:oneOf/0/properties/data/properties/items/items/required/0` | killed | killed |
| `pactling_list_integrations.output:oneOf/0/properties/data/properties/items/items/required/1` | killed | killed |
| `pactling_list_integrations.output:oneOf/0/properties/data/properties/items/items/required/2` | killed | killed |
| `pactling_list_integrations.output:oneOf/0/properties/data/properties/items/items/additionalProperties` | killed | killed |
| `pactling_list_integrations.output:oneOf/0/properties/data/properties/nextCursor/type` | killed | killed |
| `pactling_list_integrations.output:oneOf/0/properties/data/properties/retrievedAt/type` | killed | killed |
| `pactling_list_integrations.output:oneOf/0/properties/data/required/0` | killed | killed |
| `pactling_list_integrations.output:oneOf/0/properties/data/required/1` | killed | killed |
| `pactling_list_integrations.output:oneOf/0/properties/data/required/2` | killed | killed |
| `pactling_list_integrations.output:oneOf/0/properties/data/additionalProperties` | killed | killed |
| `pactling_list_integrations.output:oneOf/0/properties/coverage/const` | killed | killed |
| `pactling_list_integrations.output:oneOf/0/properties/evidenceLinks/type` | killed | killed |
| `pactling_list_integrations.output:oneOf/0/properties/evidenceLinks/items/type` | survived | killed |
| `pactling_list_integrations.output:oneOf/0/properties/evidenceLinks/items/properties/rel/type` | killed | killed |
| `pactling_list_integrations.output:oneOf/0/properties/evidenceLinks/items/properties/href/type` | killed | killed |
| `pactling_list_integrations.output:oneOf/0/properties/evidenceLinks/items/required/0` | killed | killed |
| `pactling_list_integrations.output:oneOf/0/properties/evidenceLinks/items/required/1` | killed | killed |
| `pactling_list_integrations.output:oneOf/0/properties/evidenceLinks/items/additionalProperties` | killed | killed |
| `pactling_list_integrations.output:oneOf/0/required/0` | killed | killed |
| `pactling_list_integrations.output:oneOf/0/required/1` | killed | killed |
| `pactling_list_integrations.output:oneOf/0/required/2` | killed | killed |
| `pactling_list_integrations.output:oneOf/0/required/3` | killed | killed |
| `pactling_list_integrations.output:oneOf/0/additionalProperties` | killed | killed |
| `pactling_list_integrations.output:oneOf/1/type` | survived | removed: redundant |
| `pactling_list_integrations.output:oneOf/1/properties/adapterSchemaVersion/const` | survived | killed |
| `pactling_list_integrations.output:oneOf/1/properties/error/type` | survived | killed |
| `pactling_list_integrations.output:oneOf/1/properties/error/properties/code/type` | survived | killed |
| `pactling_list_integrations.output:oneOf/1/properties/error/properties/message/type` | survived | killed |
| `pactling_list_integrations.output:oneOf/1/properties/error/properties/retryable/type` | survived | killed |
| `pactling_list_integrations.output:oneOf/1/properties/error/properties/httpStatus/type` | survived | killed |
| `pactling_list_integrations.output:oneOf/1/properties/error/properties/evidenceUrl/type` | survived | killed |
| `pactling_list_integrations.output:oneOf/1/properties/error/properties/details/type` | survived | killed |
| `pactling_list_integrations.output:oneOf/1/properties/error/properties/details/properties/reasonCode/type` | survived | killed |
| `pactling_list_integrations.output:oneOf/1/properties/error/properties/details/properties/field/type` | survived | killed |
| `pactling_list_integrations.output:oneOf/1/properties/error/properties/details/required/0` | survived | killed |
| `pactling_list_integrations.output:oneOf/1/properties/error/properties/details/additionalProperties` | survived | killed |
| `pactling_list_integrations.output:oneOf/1/properties/error/required/0` | killed | killed |
| `pactling_list_integrations.output:oneOf/1/properties/error/required/1` | killed | killed |
| `pactling_list_integrations.output:oneOf/1/properties/error/required/2` | killed | killed |
| `pactling_list_integrations.output:oneOf/1/properties/error/additionalProperties` | survived | killed |
| `pactling_list_integrations.output:oneOf/1/required/0` | survived | killed |
| `pactling_list_integrations.output:oneOf/1/required/1` | survived | killed |
| `pactling_list_integrations.output:oneOf/1/additionalProperties` | survived | killed |
| `pactling_list_pacticipants.input:type` | survived | killed |
| `pactling_list_pacticipants.input:properties/pageSize/type` | killed | killed |
| `pactling_list_pacticipants.input:properties/pageSize/minimum` | killed | killed |
| `pactling_list_pacticipants.input:properties/pageSize/maximum` | killed | killed |
| `pactling_list_pacticipants.input:properties/cursor/type` | killed | killed |
| `pactling_list_pacticipants.input:properties/cursor/minLength` | killed | killed |
| `pactling_list_pacticipants.input:additionalProperties` | killed | killed |
| `pactling_list_pacticipants.input:not` | killed | killed |
| `pactling_list_pacticipants.input:not/required/0` | killed | killed |
| `pactling_list_pacticipants.input:not/required/1` | killed | killed |
| `pactling_list_pacticipants.output:type` | survived | killed |
| `pactling_list_pacticipants.output:oneOf` | killed | killed |
| `pactling_list_pacticipants.output:oneOf/0/type` | survived | removed: redundant |
| `pactling_list_pacticipants.output:oneOf/0/properties/adapterSchemaVersion/const` | killed | killed |
| `pactling_list_pacticipants.output:oneOf/0/properties/data/type` | killed | killed |
| `pactling_list_pacticipants.output:oneOf/0/properties/data/properties/items/type` | killed | killed |
| `pactling_list_pacticipants.output:oneOf/0/properties/data/properties/items/items/type` | survived | killed |
| `pactling_list_pacticipants.output:oneOf/0/properties/data/properties/items/items/properties/name/type` | killed | killed |
| `pactling_list_pacticipants.output:oneOf/0/properties/data/properties/items/items/properties/href/type` | killed | killed |
| `pactling_list_pacticipants.output:oneOf/0/properties/data/properties/items/items/properties/latestVersion/type` | killed | killed |
| `pactling_list_pacticipants.output:oneOf/0/properties/data/properties/items/items/required/0` | killed | killed |
| `pactling_list_pacticipants.output:oneOf/0/properties/data/properties/items/items/required/1` | killed | killed |
| `pactling_list_pacticipants.output:oneOf/0/properties/data/properties/items/items/required/2` | killed | killed |
| `pactling_list_pacticipants.output:oneOf/0/properties/data/properties/items/items/additionalProperties` | killed | killed |
| `pactling_list_pacticipants.output:oneOf/0/properties/data/properties/nextCursor/type` | killed | killed |
| `pactling_list_pacticipants.output:oneOf/0/properties/data/properties/retrievedAt/type` | killed | killed |
| `pactling_list_pacticipants.output:oneOf/0/properties/data/required/0` | killed | killed |
| `pactling_list_pacticipants.output:oneOf/0/properties/data/required/1` | killed | killed |
| `pactling_list_pacticipants.output:oneOf/0/properties/data/required/2` | killed | killed |
| `pactling_list_pacticipants.output:oneOf/0/properties/data/additionalProperties` | killed | killed |
| `pactling_list_pacticipants.output:oneOf/0/properties/coverage/const` | killed | killed |
| `pactling_list_pacticipants.output:oneOf/0/properties/evidenceLinks/type` | killed | killed |
| `pactling_list_pacticipants.output:oneOf/0/properties/evidenceLinks/items/type` | survived | killed |
| `pactling_list_pacticipants.output:oneOf/0/properties/evidenceLinks/items/properties/rel/type` | killed | killed |
| `pactling_list_pacticipants.output:oneOf/0/properties/evidenceLinks/items/properties/href/type` | killed | killed |
| `pactling_list_pacticipants.output:oneOf/0/properties/evidenceLinks/items/required/0` | killed | killed |
| `pactling_list_pacticipants.output:oneOf/0/properties/evidenceLinks/items/required/1` | killed | killed |
| `pactling_list_pacticipants.output:oneOf/0/properties/evidenceLinks/items/additionalProperties` | killed | killed |
| `pactling_list_pacticipants.output:oneOf/0/required/0` | killed | killed |
| `pactling_list_pacticipants.output:oneOf/0/required/1` | killed | killed |
| `pactling_list_pacticipants.output:oneOf/0/required/2` | killed | killed |
| `pactling_list_pacticipants.output:oneOf/0/required/3` | killed | killed |
| `pactling_list_pacticipants.output:oneOf/0/additionalProperties` | killed | killed |
| `pactling_list_pacticipants.output:oneOf/1/type` | survived | removed: redundant |
| `pactling_list_pacticipants.output:oneOf/1/properties/adapterSchemaVersion/const` | survived | killed |
| `pactling_list_pacticipants.output:oneOf/1/properties/error/type` | survived | killed |
| `pactling_list_pacticipants.output:oneOf/1/properties/error/properties/code/type` | survived | killed |
| `pactling_list_pacticipants.output:oneOf/1/properties/error/properties/message/type` | survived | killed |
| `pactling_list_pacticipants.output:oneOf/1/properties/error/properties/retryable/type` | survived | killed |
| `pactling_list_pacticipants.output:oneOf/1/properties/error/properties/httpStatus/type` | survived | killed |
| `pactling_list_pacticipants.output:oneOf/1/properties/error/properties/evidenceUrl/type` | survived | killed |
| `pactling_list_pacticipants.output:oneOf/1/properties/error/properties/details/type` | survived | killed |
| `pactling_list_pacticipants.output:oneOf/1/properties/error/properties/details/properties/reasonCode/type` | survived | killed |
| `pactling_list_pacticipants.output:oneOf/1/properties/error/properties/details/properties/field/type` | survived | killed |
| `pactling_list_pacticipants.output:oneOf/1/properties/error/properties/details/required/0` | survived | killed |
| `pactling_list_pacticipants.output:oneOf/1/properties/error/properties/details/additionalProperties` | survived | killed |
| `pactling_list_pacticipants.output:oneOf/1/properties/error/required/0` | killed | killed |
| `pactling_list_pacticipants.output:oneOf/1/properties/error/required/1` | killed | killed |
| `pactling_list_pacticipants.output:oneOf/1/properties/error/required/2` | killed | killed |
| `pactling_list_pacticipants.output:oneOf/1/properties/error/additionalProperties` | survived | killed |
| `pactling_list_pacticipants.output:oneOf/1/required/0` | survived | killed |
| `pactling_list_pacticipants.output:oneOf/1/required/1` | survived | killed |
| `pactling_list_pacticipants.output:oneOf/1/additionalProperties` | survived | killed |
| `pactling_matrix.input:type` | killed | killed |
| `pactling_matrix.input:properties/participantA/type` | killed | killed |
| `pactling_matrix.input:properties/participantA/minLength` | killed | killed |
| `pactling_matrix.input:properties/participantA/maxLength` | killed | killed |
| `pactling_matrix.input:properties/participantB/type` | killed | killed |
| `pactling_matrix.input:properties/participantB/minLength` | killed | killed |
| `pactling_matrix.input:properties/participantB/maxLength` | killed | killed |
| `pactling_matrix.input:properties/latestby/enum` | killed | killed |
| `pactling_matrix.input:required/0` | killed | killed |
| `pactling_matrix.input:required/1` | killed | killed |
| `pactling_matrix.input:additionalProperties` | killed | killed |
| `pactling_matrix.output:type` | survived | killed |
| `pactling_matrix.output:oneOf` | killed | killed |
| `pactling_matrix.output:oneOf/0/type` | survived | removed: redundant |
| `pactling_matrix.output:oneOf/0/properties/adapterSchemaVersion/const` | killed | killed |
| `pactling_matrix.output:oneOf/0/properties/data/type` | killed | killed |
| `pactling_matrix.output:oneOf/0/properties/data/properties/summary/type` | killed | killed |
| `pactling_matrix.output:oneOf/0/properties/data/properties/summary/properties/deployable/type` | killed | killed |
| `pactling_matrix.output:oneOf/0/properties/data/properties/summary/properties/reason/type` | killed | killed |
| `pactling_matrix.output:oneOf/0/properties/data/properties/summary/properties/success/type` | killed | killed |
| `pactling_matrix.output:oneOf/0/properties/data/properties/summary/properties/success/minimum` | killed | killed |
| `pactling_matrix.output:oneOf/0/properties/data/properties/summary/properties/failed/type` | killed | killed |
| `pactling_matrix.output:oneOf/0/properties/data/properties/summary/properties/failed/minimum` | killed | killed |
| `pactling_matrix.output:oneOf/0/properties/data/properties/summary/properties/unknown/type` | killed | killed |
| `pactling_matrix.output:oneOf/0/properties/data/properties/summary/properties/unknown/minimum` | killed | killed |
| `pactling_matrix.output:oneOf/0/properties/data/properties/summary/required/0` | killed | killed |
| `pactling_matrix.output:oneOf/0/properties/data/properties/summary/required/1` | killed | killed |
| `pactling_matrix.output:oneOf/0/properties/data/properties/summary/required/2` | killed | killed |
| `pactling_matrix.output:oneOf/0/properties/data/properties/summary/required/3` | killed | killed |
| `pactling_matrix.output:oneOf/0/properties/data/properties/summary/required/4` | killed | killed |
| `pactling_matrix.output:oneOf/0/properties/data/properties/notices/type` | killed | killed |
| `pactling_matrix.output:oneOf/0/properties/data/properties/notices/items/type` | survived | killed |
| `pactling_matrix.output:oneOf/0/properties/data/properties/notices/items/properties/type` | killed | killed |
| `pactling_matrix.output:oneOf/0/properties/data/properties/notices/items/properties/type/type` | killed | killed |
| `pactling_matrix.output:oneOf/0/properties/data/properties/notices/items/properties/text/type` | killed | killed |
| `pactling_matrix.output:oneOf/0/properties/data/properties/notices/items/required/0` | killed | killed |
| `pactling_matrix.output:oneOf/0/properties/data/properties/notices/items/required/1` | killed | killed |
| `pactling_matrix.output:oneOf/0/properties/data/properties/matrix/type` | killed | killed |
| `pactling_matrix.output:oneOf/0/properties/data/properties/matrix/items/type` | survived | killed |
| `pactling_matrix.output:oneOf/0/properties/data/properties/matrix/items/properties/consumer/type` | killed | killed |
| `pactling_matrix.output:oneOf/0/properties/data/properties/matrix/items/properties/consumer/properties/name/type` | killed | killed |
| `pactling_matrix.output:oneOf/0/properties/data/properties/matrix/items/properties/consumer/properties/version/type` | killed | killed |
| `pactling_matrix.output:oneOf/0/properties/data/properties/matrix/items/properties/consumer/required/0` | killed | killed |
| `pactling_matrix.output:oneOf/0/properties/data/properties/matrix/items/properties/consumer/required/1` | killed | killed |
| `pactling_matrix.output:oneOf/0/properties/data/properties/matrix/items/properties/provider/type` | killed | killed |
| `pactling_matrix.output:oneOf/0/properties/data/properties/matrix/items/properties/provider/properties/name/type` | killed | killed |
| `pactling_matrix.output:oneOf/0/properties/data/properties/matrix/items/properties/provider/properties/version/type` | killed | killed |
| `pactling_matrix.output:oneOf/0/properties/data/properties/matrix/items/properties/provider/required/0` | killed | killed |
| `pactling_matrix.output:oneOf/0/properties/data/properties/matrix/items/properties/provider/required/1` | killed | killed |
| `pactling_matrix.output:oneOf/0/properties/data/properties/matrix/items/properties/pact/type` | killed | killed |
| `pactling_matrix.output:oneOf/0/properties/data/properties/matrix/items/properties/verificationResult/type` | killed | killed |
| `pactling_matrix.output:oneOf/0/properties/data/properties/matrix/items/required/0` | killed | killed |
| `pactling_matrix.output:oneOf/0/properties/data/properties/matrix/items/required/1` | killed | killed |
| `pactling_matrix.output:oneOf/0/properties/data/properties/matrix/items/required/2` | killed | killed |
| `pactling_matrix.output:oneOf/0/properties/data/properties/matrix/items/required/3` | killed | killed |
| `pactling_matrix.output:oneOf/0/properties/data/required/0` | killed | killed |
| `pactling_matrix.output:oneOf/0/properties/data/required/1` | killed | killed |
| `pactling_matrix.output:oneOf/0/properties/data/required/2` | killed | killed |
| `pactling_matrix.output:oneOf/0/properties/coverage/const` | killed | killed |
| `pactling_matrix.output:oneOf/0/properties/evidenceLinks/type` | killed | killed |
| `pactling_matrix.output:oneOf/0/properties/evidenceLinks/items/type` | survived | killed |
| `pactling_matrix.output:oneOf/0/properties/evidenceLinks/items/properties/rel/type` | killed | killed |
| `pactling_matrix.output:oneOf/0/properties/evidenceLinks/items/properties/href/type` | killed | killed |
| `pactling_matrix.output:oneOf/0/properties/evidenceLinks/items/required/0` | killed | killed |
| `pactling_matrix.output:oneOf/0/properties/evidenceLinks/items/required/1` | killed | killed |
| `pactling_matrix.output:oneOf/0/properties/evidenceLinks/items/additionalProperties` | killed | killed |
| `pactling_matrix.output:oneOf/0/required/0` | killed | killed |
| `pactling_matrix.output:oneOf/0/required/1` | killed | killed |
| `pactling_matrix.output:oneOf/0/required/2` | killed | killed |
| `pactling_matrix.output:oneOf/0/required/3` | killed | killed |
| `pactling_matrix.output:oneOf/0/additionalProperties` | killed | killed |
| `pactling_matrix.output:oneOf/1/type` | survived | removed: redundant |
| `pactling_matrix.output:oneOf/1/properties/adapterSchemaVersion/const` | survived | killed |
| `pactling_matrix.output:oneOf/1/properties/error/type` | survived | killed |
| `pactling_matrix.output:oneOf/1/properties/error/properties/code/type` | survived | killed |
| `pactling_matrix.output:oneOf/1/properties/error/properties/message/type` | survived | killed |
| `pactling_matrix.output:oneOf/1/properties/error/properties/retryable/type` | survived | killed |
| `pactling_matrix.output:oneOf/1/properties/error/properties/httpStatus/type` | survived | killed |
| `pactling_matrix.output:oneOf/1/properties/error/properties/evidenceUrl/type` | survived | killed |
| `pactling_matrix.output:oneOf/1/properties/error/properties/details/type` | survived | killed |
| `pactling_matrix.output:oneOf/1/properties/error/properties/details/properties/reasonCode/type` | survived | killed |
| `pactling_matrix.output:oneOf/1/properties/error/properties/details/properties/field/type` | survived | killed |
| `pactling_matrix.output:oneOf/1/properties/error/properties/details/required/0` | survived | killed |
| `pactling_matrix.output:oneOf/1/properties/error/properties/details/additionalProperties` | survived | killed |
| `pactling_matrix.output:oneOf/1/properties/error/required/0` | killed | killed |
| `pactling_matrix.output:oneOf/1/properties/error/required/1` | killed | killed |
| `pactling_matrix.output:oneOf/1/properties/error/required/2` | killed | killed |
| `pactling_matrix.output:oneOf/1/properties/error/additionalProperties` | survived | killed |
| `pactling_matrix.output:oneOf/1/required/0` | survived | killed |
| `pactling_matrix.output:oneOf/1/required/1` | survived | killed |
| `pactling_matrix.output:oneOf/1/additionalProperties` | survived | killed |


## Final verification and handoff

All final commands exit 0. Source default: **256 tests passed**; MCP feature:
**317 tests passed**, including both compile-fail client-boundary doctests. No tests ignored.
Both all-target clippy checks pass with warnings denied. `cargo fmt --check` is silent.
The final additional unit test covers 400 environment-error recognition for 1.0/1.9/1.100,
and safe fallback for 2.0, malformed minor text, empty, null and numeric version values.
All existing fixture bytes, the original explanation schema and the original committed
`tests/mcp_tools.rs` are unchanged. Generated executables were cleaned only after tests finished;
source checkouts and `/tmp/pactling-mcp-baseline` remain untouched.

Commands (the same environment applies to every cargo invocation):

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

Raw complete logs:
- `/tmp/pactling-task3-fmt-final.log`
- `/tmp/pactling-task3-clippy-default-final.log`
- `/tmp/pactling-task3-full-default-final.log`
- `/tmp/pactling-task3-clippy-mcp-final.log`
- `/tmp/pactling-task3-full-mcp-final.log`
- `/tmp/pactling-task3-verification.json`

Literal excerpts:

```text
    Finished `dev` profile [unoptimized] target(s) in 5.91s
    Finished `dev` profile [unoptimized] target(s) in 6.57s
test result: ok. 55 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.08s
test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.17s
test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.07s
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s
test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.39s
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.11s
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.08s
test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.05s
test result: ok. 20 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.16s
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.05s
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.21s
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.68s
```

Task 4 can build directly on this branch after merge. It should replace the two explicitly
transitional tests with frozen-cache behavior, preserve all other fidelity assertions,
add the final complete-result bound/cancellation/parity checks, and retain fixed safe error
messages. No independent review is claimed here; the feature-level non-local review remains
required before any push.
