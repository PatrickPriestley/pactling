# Read-only HTML UI

Open `http://localhost:9292/ui/` while the normal `pactling` server is running. The UI is
served by the same binary with embedded styles and works with JavaScript disabled.

| Page | GET route | Purpose |
| --- | --- | --- |
| Integrations | `/ui/` | All published integration pairs, each linked to its latest exact-version pact and matrix |
| Pact | `/ui/pacts/provider/{p}/consumer/{c}/version/{v}` | Exact consumer-version publication, interactions and expandable full contract |
| Matrix | `/ui/matrix/provider/{p}/consumer/{c}` | Pair rows, version filters, latestby grouping, verification and ignored state |
| Deployment explanation | `/ui/deployment-explanation` | Selector/target form, verdict, reasons, suggested actions and recorded evidence |

The index has no search or pagination in v1. Publishing, deployment recording,
webhook administration and other mutations remain API/client operations. Suggested actions
are descriptions and evidence links, not buttons that perform writes.

## Queries and evidence

The matrix form supports all versions, exact version, latest, latest tagged version,
branch head and main-branch head independently for each side. Grouping uses the existing
`latestby` semantics. Submitted form selections remain visible after filtering.
The route fixes the pair: unrelated or excess selectors are rejected; self-integrations
are supported. Direct matrix-shaped queries preserve explicit selector order and append
only a missing member of the route's pair.

The explanation form selects a pacticipant, version mode and target environment or tag.
Optional ignores use `NAME` or `NAME=VERSION`; submitting preserves populated entries and
provides one blank entry for another ignore. Marked native forms omit unused inputs before
calling the existing parser. Empty unused version fields therefore cannot override a
main-branch choice. Direct queries use the [explanation API grammar](deployment-explanations.md)
unchanged, including its unpinned selectors and repeated ignores, for example:

```
/ui/deployment-explanation?pacticipant=Consumer&version=1.0.0&environment=production
/ui/deployment-explanation?q[][pacticipant]=Consumer&q[][mainBranch]=true&q[][latest]=true&environment=test
```

For direct queries the form is labeled **New explanation**: its convenient prefills do
not represent every possible multi-selector query. The evaluated query is shown separately.
The simple native form intentionally creates a new single-selector request; it does not
silently change the displayed decision. The same distinction applies to direct matrix queries.

Each explanation uses one shared `DecisionEvaluation`, the same document projection as
the JSON API, and the existing deferred read snapshot. It shows allowed, blocked or unknown;
unknown is never presented as allowed. Reasons retain their roles and open codes, with exact
next-action targets and evidence links. Candidate provenance is shown separately from the
matrix's null provider versions. Expand referenced evidence for detailed resolution and
verification timestamps. Evidence links navigate to the corresponding HTML records.

`evaluatedAt`, `recorded_only` and `not_checked` describe the snapshot and its limits:
this is recorded broker evidence, not a live environment-health check or a freshness guarantee.
There is no age budget. Later publications or deployments may change a later evaluation.

## Authentication and compatibility

All pages and `/ui/style.css` inherit the existing broker read authorization: configured
Basic credentials (including the read-only user), bearer credentials, or configured public
reads. With no credentials configured the broker remains open. There is no second login
system and credentials never belong in UI query strings.

Pages support GET and OPTIONS; other methods receive 405 after normal authentication
checks (unauthorized callers retain the existing 401 behavior). Missing addressed pacts
return HTML 404; query validation returns HTML 400. Internal failures use generic HTML
messages. Page responses are `no-store`. All user/stored text is escaped; evidence links
allow local paths or HTTP(S), with safe attributes on external links.

The additive `/ui/` namespace deliberately differs from Ruby's HTML negotiation on API
paths. Root discovery, pact, matrix, can-i-deploy and explanation JSON routes retain their
status, document and Accept behavior. Existing fixtures are unchanged. Leptos/WASM (#15),
JavaScript, live updates and a full Ruby UI clone are outside this version.
