# Matrix latestby fidelity (#23)

Goal: apply the already parsed latestby option before matrix ignores and verdicts,
matching the pinned Ruby broker 2.121.2. Work only in this clone on fix/matrix-latestby.

Evidence: read Matrix::Repository#apply_latestby and its query ordering directly from
pactfoundation/pact-broker@sha256:420341d4fbfb00a621c313f47e530ad75703f969bbc7520d0d72bc71bb53cef9.
The running fresh pactling-probe Compose broker reports X-Pact-Broker-Version 2.121.2.
Ruby groups by consumer/version/provider/version (cvpv), consumer/version/provider
(cvp), or consumer/provider (cp). An absent option leaves rows alone. The first null
provider row wins; otherwise stable provider_version_order sorting chooses the last
row. Grouping precedes ignores. Query order is last-action date, pact order, then
verification ID, descending. These are exact-source facts; capture requests will
verify their observable effects before implementation.

Plan: extend capture-matrix-regressions.py with an optional latestby and isolated
MatrixLatest* services. Capture deployed-new-success/released-old-failure from both
sides and two selectors for every mode and absence; also cover grouping identities,
provider creation order, equal-version ties, unverified first rows, and ignores.
Keep the original 73 projections unchanged. Replay in tests/matrix_regressions.rs and
record assertion failures before touching the service. Implement one post-query
reduction in src/service/matrix.rs, rather than special casing environment queries.
Use version IDs for provider creation order, never numeric/semantic version strings.

Checks: fresh Ruby capture and recapture comparison; regression replay and all fixture
tests; mutation of each new guard; fmt, clippy, cargo test and full real-client e2e;
non-Codex completed independent review before push and a PR closing #23. Stop the
probe broker when finished. No main merge.

## Evidence and validation

- Fresh real Ruby capture: 108 cases, including 35 additions; the original 73
  expected projections are unchanged. Fresh recapture comparison: zero differences.
- Before service changes, 16 new cvp/cp cases failed on row/summary assertions;
  cvpv and absent-option controls already passed. Regression replay is now green.
- Grouping executes once after overlapping-selector deduplication and before ignore
  flags/verdict computation. Provider IDs are creation order; versions 99 then 2 in
  the capture prove this is not lexical/semantic version sorting.
- Mutations killed: each of the three grouping modes, collapsing an absent option,
  replacing a null-first row, selecting the oldest provider, retaining the first
  equal-order row, omitting consumer identity, and omitting provider identity.
  Removing query ordering initially survived. Added real Ruby split-selector cases;
  the removal now fails, and the ordering was restored.
- Exact-source follow-up: MatrixRow's misleading provider_version_created_at alias
  comes from LatestVerificationIdForPactVersionAndProviderVersion.created_at, populated
  with verification.created_at (verifications/repository.rb:39-47). Consumer-side
  cache uses consumer_version.created_at (pacts/repository.rb:89-97). Added a pure
  service regression with distinct dates, red before correcting that field mapping.
  This date-specific assertion is source-supported, not a captured HTTP scenario.
- Additional cases use isolated MatrixLatest* participants so all setup can be replayed
  before queries. Provider-side cases verify each selected provider/consumer pair to
  isolate grouping from unrelated unknown-row/description behavior.
