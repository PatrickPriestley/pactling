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
