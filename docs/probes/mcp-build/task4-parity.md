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
