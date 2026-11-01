# Minimal UI reference captures — 2026-11-01

These are documentation evidence, not new compatibility fixtures or implemented UI.
All seeded names/content are synthetic. No credentials were captured.

## Reproduce

Start `docker compose -p pactling-probe -f scripts/ruby-broker/docker-compose.yml up -d`.
The image is pinned to `sha256:420341d4fbfb00a621c313f47e530ad75703f969bbc7520d0d72bc71bb53cef9`;
responses report broker 2.121.2. Use a **fresh scratch database**, not an existing fixture DB:
create a uniquely named database with compose `exec -T postgres createdb -U pactbroker NAME`,
and add a temporary compose override setting `pact-broker.environment.PACT_BROKER_DATABASE_NAME`
to NAME. Run `up -d` with both files. This capture preserved the original database and used
`pactbroker_ui_plan_20261101_1530`. Do not delete shared volumes.

Run `python3 docs/reference/capture-minimal-ui.py http://localhost:9292 docs/reference/ruby-broker/2.121.2/minimal-ui`.
This seeds two integrations, three distinct UiCheckout contracts (success, failure,
unverified), provider UiPayments version 2 deployed to ui-test, then probes eight Accept
variants on eleven paths. `seed.json` records writes, `negotiation.json` records requests,
status, content type, Vary and raw-body SHA-256; each referenced body is retained.
`api-after-html.json` compares the HAL/JSON responses before and after HTML requests.
Stop the compose project after use; never run the seed twice on the same database.

The same script ran against `cargo build`'s real pactling binary at
`811097d7162df688c7c2bdae76f3d6a2ce2fe2ae`, started with
`target/debug/pactling --port 9393 --database-url sqlite::memory:`.
Results are in `pactling/minimal-ui/`. All twenty compatibility comparisons were
byte-identical. The extension changes evaluation ID, startedAt and evaluatedAt on each
request; `explanation-comparison.json` records equality of the HAL-Accept and HTML-Accept
documents excluding **only those three paths**. This is a baseline, not proof of future UI code.

## Browser reference

Headless system Chrome, Playwright-core, viewport 1280×900, default browser Accept,
`goto(..., waitUntil: networkidle)`, `screenshot(fullPage: true)` and `page.content()`:

| Capture | Ruby path | Observed content |
| --- | --- | --- |
| [Index](ruby-broker/2.121.2/minimal-ui/index.png) | `/` | Integration pairs, latest pact and matrix links, verification indicator |
| [Pact](ruby-broker/2.121.2/minimal-ui/pact.png) | `/pacts/provider/UiPayments/consumer/UiCheckout/version/1` | Consumer version, interaction state, request and response |
| [Matrix](ruby-broker/2.121.2/minimal-ui/matrix.png) | `/matrix/provider/UiPayments/consumer/UiCheckout` | Three rows: missing, failed and successful verification; branch/environment labels |
| [Dashboard](ruby-broker/2.121.2/minimal-ui/dashboard.png) | `/dashboard/provider/UiPayments/consumer/UiCheckout` | Publication list; contextual reference only, not a fifth proposed page |

Raw HTML, `*.rendered.html` and `browser-navigation.json` accompany screenshots.
Screenshots were visually inspected. Ruby's pact badge can show the integration's latest
failed verification even on the version-1 pact; it is not an exact-contract deployment verdict.
Ruby has no pactling explanation endpoint (404); `/can-i-deploy` returns 406 for text/html.
The matrix provides visual reference for readiness, not an equivalent explanation page.

## Client dependencies and limits

The browser navigation log verifies HTML negotiation on shared Ruby paths. Existing
`tests/fixtures/cli/phase4/*.jsonl` records the real pact-broker client's
`Accept: application/hal+json` and HAL link traversal. The pact endpoint additionally serves
`application/json`; preserve its existing pactling behavior in `src/http/pacts.rs::render_pact`.
No claim is made that every Ruby UI feature or every client/header combination was tested.
Captures include dynamic dates, UUIDs and local URLs; they are reference evidence, not
byte-for-byte golden HTML tests. No new runtime dependency or production code was added.
