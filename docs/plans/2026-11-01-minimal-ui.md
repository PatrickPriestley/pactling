---
status: accepted
accepted_by: Patrick Priestley
accepted_on: 2026-11-01
---

# Minimal read-only server-rendered UI

## Goal and scope

Let a person inspect integrations, an exact pact, a pair's matrix and a deployment
explanation without constructing API requests. Serve four plain-HTML pages from the
existing axum binary, using existing data and decision services. This is a proposal only;
no production code is included. Publishing, deployment recording, administration, a full
Ruby UI clone, dashboards and live updates are out of scope. [Issue #15](https://github.com/PatrickPriestley/pactling/issues/15)
(Leptos/WASM) stays deferred; no JS framework or JavaScript is required.

## Probed facts

**Verified against real systems:** [capture instructions, HTML, screenshots and manifests](../reference/README-minimal-ui.md)
record Ruby 2.121.2 and pactling main `811097d` with the same synthetic data.

| Path family | Ruby `text/html` | Ruby `application/hal+json` | Ruby `application/json` |
| --- | --- | --- | --- |
| `/` | 200 HTML integration index | 200 HAL | 406 |
| `/pacts/provider/{p}/consumer/{c}/version/{v}` and `/latest` | 200 HTML pact | 200 HAL | 200 JSON |
| `/matrix` and `/matrix/provider/{p}/consumer/{c}` | 200 HTML matrix | 200 HAL | 406 |
| `/dashboard/provider/{p}/consumer/{c}` | 200 HTML publications | 200 HAL | 406 |
| `/can-i-deploy` | 406 | 200 HAL | 406 |
| `/integrations` | 406 | 200 HAL | 406 |

Ruby's `/integrations` defaults to Graphviz, not its HTML index. Browser-style Accept
selects HTML on the index, pact and matrix. Existing recorded real CLI traffic depends on
HAL discovery and matrix links; changing default representations would break that contract.
Current pactling returns its API representations even for HTML Accept on those paths;
its dashboard is 404. Both brokers' compatibility HAL/JSON responses were byte-identical
before/after HTML GETs. Pactling's explanation differs only in its evaluation ID/times
when compared across Accept variants (details in evidence). These are baseline probes,
not a claim that unbuilt UI handlers preserve compatibility.

**Supported by exact source:** `src/service/matrix.rs::decision` reads through
`Db::begin_read`; `src/service/deployment_explanations.rs::explain` projects its evaluation.
`src/http/deployment_explanations.rs` owns query validation and evidence-link rendering.
Reuse those seams; HTML must never calculate an independent verdict.

## Decisions (and rejected alternatives)

- **Maud**, with `Html(markup.into_string())`, embedded CSS and no runtime template files.
  Its [documentation](https://maud.lambda.xyz/) supports Rust-value rendering and
  [default escaping](https://maud.lambda.xyz/text-escaping.html); dependency/build compatibility
  remains **unprobed** until implementation. Reject [Askama](https://askama.rs/en/stable/):
  compile-time file templates are valid, but four small pages do not need separate template
  context structs/files. Reject Leptos, client-side rendering and remote CDN assets.
- **Additive `/ui/` namespace**, not negotiation changes to existing routes. Proposed GETs:
  `/ui/` (integration index), `/ui/pacts/provider/{p}/consumer/{c}/version/{v}`,
  `/ui/matrix/provider/{p}/consumer/{c}`, `/ui/deployment-explanation`.
  Reject replacing `/`, adding root HAL links, or shadowing Ruby HTML routes: preserve every
  existing API status, document, default and Accept behavior. Document the new namespace as
  a deliberate difference. UI links stay in `/ui/`; explicit evidence/API links are labeled.
- **Small read models, existing decisions.** Index lists pair names and links to matrix and
  latest publication's exact-version pact. Pact shows version, state, interactions and escaped
  request/response bodies; no integration-wide badge presented as exact-contract success.
  Matrix uses the named pair and existing selector/latestby semantics, row order, verification
  state and null versions. Reject new SQL decision logic or invented versions for nulls.
- **Explanation GET form uses the existing explanation query contract**, including version,
  latest/tag, branch/main-branch, target environment/to tag and repeated ignores. Share its
  validation, call `matrix::decision` once, then its existing explanation projection/document
  renderer for evidence links. Display allowed/blocked/unknown, reasons, exact action targets,
  evidence links, evaluation time and `recorded_only`/`not_checked` limits. Preserve candidate
  provenance separately from Ruby nulls and tolerate open reason/action codes. Reject HTTP
  loopback, verdict re-evaluation, age budgets and action buttons that execute writes.
- **Inherit existing broker authentication/read authorization**, including read-only credentials
  and configured public reads; no new login or credentials in URLs. New pages accept GET/OPTIONS,
  other methods return 405; no writes, webhooks or background jobs. HTML errors retain 400/404
  status, are escaped and reveal no internals; existing API errors stay unchanged. Use no-store
  for authenticated/evaluated pages. Reject a second auth system or public bypass.
- **Escape all stored/user text and encode route segments/query values.** Never apply Maud
  `PreEscaped` to broker data. Evidence links must allow only local paths or http(s), rejecting
  executable schemes; external links get safe rel attributes. Use headings, table headers,
  labeled forms, keyboard links and verdict words, not color alone. Reject copying Ruby's
  JavaScript-dependent controls or raw pact HTML. Large bodies use wrapping/scrolling, without
  silently hiding matrix rows or evidence; pagination/performance work is deferred.

## Ordered build tasks (after acceptance)

1. Probe a minimal Maud build with this repository's Rust/axum versions; update `Cargo.toml`
   and lockfile only after it works. Add failing `tests/ui.rs::ui_index_lists_integrations`
   and `ui_routes_preserve_api_negotiation`, then wire `src/http/ui/mod.rs`,
   `src/http/mod.rs`, `src/app.rs`, shared `layout.rs` and embedded `style.css`.
   Index `ui/index.rs` uses existing integration/publication DB reads; any multi-query read
   stays within `Db::begin_read` (never the write transaction).
2. Add failing `ui_pact_renders_exact_version`, `ui_escapes_untrusted_content`,
   `ui_encodes_links_and_rejects_unsafe_schemes`; implement `src/http/ui/pact.rs` using the
   existing publication lookup. Cover unusual names, empty interactions and missing pacts.
3. Add `ui_matrix_preserves_rows_and_nulls`, `ui_matrix_filters_match_api`; implement
   `src/http/ui/matrix.rs` with `MatrixQuery` and shared `decision`. Test unknown, ignored,
   failed and successful rows, latestby grouping and invalid input without altering API parsing.
4. Add `ui_explanation_matches_shared_evaluation` and `ui_explanation_validates_query` in
   `tests/ui.rs`; implement `src/http/ui/explanation.rs`. Narrowly expose existing validation
   and document projection from `src/http/deployment_explanations.rs`; do not fork them.
   Extend `tests/explanation_corpus.rs` to compare HTML view-model verdict/rows to the same
   evaluation for all 108 captured cases; HTTP tests cover allowed, blocked, unknown, mixed
   reasons, unpinned selectors, ignores and missing targets. No fixture bytes change.
5. Add `ui_respects_read_auth`, `ui_rejects_mutating_methods`, `ui_reads_do_not_write`,
   `ui_assets_are_embedded`, plus empty-index/400/404 tests. Check DB state and webhook queue
   unchanged after browsing, query_only enforcement and no pool leakage; reuse existing
   read-snapshot concurrency coverage. Mutation-check each new/touched guard, including
   auth, escaping/link validation and verdict mapping, recording each kill.
6. Document routes in README deliberate differences and `docs/api/minimal-ui.md`. Run fmt,
   clippy with `-D warnings`, all fixture/corpus/full-document tests and `bash e2e/run.sh`.
   Probe the real binary with captured Accept variants before/after UI requests, comparing
   existing responses (normalize only known dynamic evaluation fields). Browser-test all
   four pages with JS disabled, keyboard navigation and narrow/desktop widths; save screenshots
   and confirm no external asset requests. Run completed non-Codex `agent-review code`, resolve
   verified findings test-first, push and open a PR without merging.

## Acceptance and remaining questions

The build passes when all four pages work read-only, the explanation uses one shared
evaluation, and existing API fixtures and real clients are unchanged. This plan/capture PR
requires `agent-review plan`; red/green and guard mutations belong to the future build,
not this docs-only change. Accepted 2026-11-01: the maintainer confirmed the additive `/ui/` namespace. Maintainer review should confirm and
whether the deliberately small first version needs index search/pagination before release.
