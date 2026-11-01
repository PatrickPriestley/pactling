# Matrix verdict correctness

Addresses #1, #2 and #3 on branch `fix/matrix-verdict-correctness`.

## Goal

Prevent false-positive deployment verdicts caused by selector order, dropped selectors,
missing pinned versions and version-insensitive ignores. Preserve Ruby broker 2.121.2
semantics without undertaking the separate transaction or decision-engine redesign.

## Evidence

Verified against the real pinned Ruby 2.121.2 container on an isolated database:
- C@1 failing against P@1 returns false in either selector order.
- W@1 passing against P@1 plus C@1 failing against P@1 returns false with both rows.
- W@1 with missing P@999 returns false, unknown=1 and a missing-version notice.
- Ignoring missing P@999 does not hide failing P@1; Ruby emits a warning.
- Ignoring P@1 hides that row and returns true when no other dependency remains.

The reviewed local implementation returned true in the problematic cases. Additional
fixtures will record exact requests and outcomes, including controls.

## Decisions

- Evaluate all selected integration directions rather than assuming the first selector
  is the consumer. Keep existing single-selector behavior outside the corrected ignore
  handling; reject a broad engine redesign in this bug-fix branch.
- Preserve unresolved pinned versions and their negative verdict rather than widening
  them to any version.
- Apply ignores after resolving individual rows. An additional real-broker probe
  corrected the initial assumption about unverified providers: when the provider
  version is null in a matrix row, a version-specific ignore does not suppress it.
  Whole-participant ignores still apply. Reject participant-only filtering for
  version-scoped ignores and do not infer a version match from a selector alone.
- Capture real reference outcomes and replay them rather than invent expected summaries.

## Tasks and validation

- Capture reproducible regression scenarios under scripts/ and tests/fixtures/.
- Add tests/matrix_regressions.rs; observe failing assertions before implementation.
- Correct src/service/matrix.rs, with storage helpers only if needed.
- Exercise positive controls and mutation-check every changed/new guard.
- Run fmt, Clippy, full enabled tests and existing real-client e2e phases.
- Independent different-family review, resolve verified findings, push and open PR.

The two pre-existing ignored phase-4 fidelity tests remain outside this scoped fix;
completing their replay sequence and phase-4 e2e is tracked separately in #7.

## Scope boundary found during controls

A real-broker probe with two released provider versions, only one verified against
a consumer, showed a pre-existing environment-selection difference: Ruby omitted
the unverified provider row while Pactling retained it as unknown. This branch
does not change that selection behavior; broader environment fidelity remains #7.
The version-ignore regression cases cover multiple verified releases and an
explicitly pinned unverified provider, so the fixed ignore predicate is tested
without bundling an unrelated environment-selection rewrite.

## Review-driven corrections

The first independent review was complete. Real Ruby probes confirmed four findings:
warning/error ordering, latest/tag/branch/main-branch resolution on the second selector,
missing-selector notices taking precedence over failure notices, and warnings for
unknown ignored participants. Added capture cases failed before those fixes. Both
sides now use the same selector resolver; this is needed for selector correctness,
not the broader pure-engine redesign tracked in #5.

## Validation

- 49 stable request/summary/row projections captured from Ruby 2.121.2; repeat capture
  matches byte-for-byte. The script is also called by the full phase-4 capture workflow.
- 116 enabled tests pass; the two pre-existing ignored phase-4 replay tests are unchanged.
- Formatting, Clippy with warnings denied, and real-client e2e phases 1–3 pass.
- 32 deliberate guard mutations were killed by assertion failures after a passing
  baseline, in a disposable source copy with its own build cache. Checks covered
  selector directions/count, overlap deduplication, missing-version handling, all
  ignore predicates, latest/branch/tag/main-branch resolution, notice precedence,
  ignored-row ordering, environment wording, and verdict reduction.
- No temporary mutations were applied to this worktree.

## Second review disposition

The second review completed. Its suspected integration-route regression was disproved
by the real Ruby broker: that route also returns both directions. The new directional
control checks row membership, counts, and verdict; it deliberately excludes the
pre-existing unpinned-provider reason wording (Ruby says "any version", Pactling says
"version 1"), which remains broader phase-4 fidelity work. Two-missing-selector and
two-missing-version probes also matched the existing implementation, disproving the
reviewer's ordering/count assumptions. The missing consumer-branch probe is now
captured. Formatting passes. No verified important review findings remain.

## Third review (independent, Claude) and fixes

Probed against the pinned Ruby 2.121.2 container and its exact source
(`lib/pact_broker/matrix/deployment_status_summary.rb`, `resolved_selector.rb`,
`row_ignorer.rb`), then captured as 21 more cases in `matrix-regressions.json` (70 in all;
the first 49 recaptured byte-for-byte). Each case failed before its fix.

- Verdict precedence (`deployable?`): a missing specified version is false; otherwise any
  unknown row is null, even next to a failure; otherwise a failure is false. The service
  had tested failures before unknowns.
- Notice order (`reasons`): ignore warnings in request order, then the environment and
  version-number warnings, then errors. Errors are the missing-version messages in
  selector order, or else every failure message followed by every unverified message, in
  row order. The alphabetical sort is gone. The version-number warning follows Ruby's
  rule (some selector is more than a name and none has a version), and the environment
  warning is not given when a `to` tag is present.
- Missing-version wording follows `version_does_not_exist_description`: tag, branch, main
  branch ("No version of X from the main branch exists"), version, then "No pacts or
  verifications have been published for X".
- A tag, branch or main-branch selector without `latest=true` selects every version with
  that tag or in that branch, so it can no longer widen to any version and approve a
  deployment Ruby refuses. Every selected version with a verification is a row; when none
  has one, a single blank row describes the newest selected version ("a version of X with
  tag T (2)").

Mutation checks: 15 guard mutations, each killed by a captured case; the last survivor
(the subject-side missing notice for a tag selector) needed the three-selector case
`tag_without_latest_missing_first_of_two`, which was captured for it.
