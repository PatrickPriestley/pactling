# Task 4 parity characterization

Tests added only to `tests/explanation_corpus.rs`; the existing all-108 shared-evaluation
and original-HTTP-grammar test and fixture bytes remain unchanged.

- `mcp_explanation_corpus_subset_is_complete` derives all 108 inclusion/exclusion rows
  from the original captured raw query, compares the complete committed manifest,
  enforces its floor of three, and asserts exactly three canonical cases. A Python
  independent reproduction of the same query-shape predicates matched all 108 rows
  before writing tests. These predicates describe adapter representability, not verdicts.
- `mcp_three_canonical_corpus_cases_match_real_broker_documents` replays the committed
  setup into a real broker, spawns the production executable over stdio, calls exactly
  the three canonical eligible cases, verifies identical query bytes, and checks the
  original Ruby expected rows/verdict through the existing corpus assertion helper.
- `mcp_selector_modes_verdicts_ignores_and_errors_match_real_broker` covers latest,
  latest tagged, branch, mainBranch, environment/tag targets, all three decisions,
  whole-participant ignores, and recognized/fallback broker query errors.

The forwarding proxy records the exact response bytes consumed by MCP; every successful
MCP data document must equal that JSON completely, including timestamps/evaluation IDs,
request, selectors, rows, reasons/actions and evidence links. No normalization and no
second broker evaluation. Every call asserts exactly one actual GET. Broker errors are
compared to the safe adapter mapping, rather than incorrectly expecting raw error forwarding.

These are characterization tests of the existing task 3 adapter. They do not constitute
red/green evidence for new pagination or claim all 108 cases as MCP round trips.
Execution and final verification are owned by the task 4 conductor; no cargo was run here.

## Characterization fixture correction

The conductor's initial run passed the manifest and three-canonical-case tests, then
failed the dedicated blocked assertion: MatrixConsumer v1 against matrix-regressions
was actually allowed. This was a test setup assumption, not adapter behavior: canonical
`latestby=cvp` selects the newer successful provider evidence in that multi-version target.
The existing Ruby `single_provider_environment` fixture records one failed provider in
`matrix-single`. A real production broker probe replayed the unchanged corpus setup and
confirmed the exact MCP query form (`latestby=cvp`) returns blocked for that target,
while the original target returns allowed. It also verified latest MatrixLatestUnknownConsumer
selects version 3 and returns unknown. Raw probe script and log:
`/tmp/pactling-task4-parity-probe.py`, `/tmp/pactling-task4-parity-probe.log`.

With conductor authorization for test hygiene, change only that dedicated target to
`matrix-single`, retaining the required blocked assertion. Use the same target for the
whole-provider ignore case, so the expected allowed outcome exercises ignored failed evidence.
Existing captured fixture bytes, manifest, and the original 108-case test stay unchanged.
