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
- Apply ignores after resolving individual rows, using the resolved version even when
  an unverified provider is rendered as null for compatibility. Reject participant-only
  filtering for version-scoped ignores.
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
