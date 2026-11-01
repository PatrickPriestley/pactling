# Plan: pactling explain CLI

Implement the maintainer-approved CLI over the v1 explanation endpoint, based on main after #31 merged. No-subcommand invocations retain the existing server parser and startup. The explanation route alone accepts the broader matrix selector grammar plus a target, as approved on 2026-11-01; compatibility routes and existing fixture bytes stay unchanged.

## Probed evidence

Verified with the installed pact-broker CLI 1.77.0, its actual help and `e2e/proxy.js` forwarding to the real baseline pactling binary: pinned, latest, latest-tag, branch, main-branch, version-specific ignore and target-tag requests. Recorded request paths are in `tests/fixtures/cli/explain-can-i-deploy-queries.json`. Environment base URL, basic authentication and bearer-token environment handling were observed on the real requests. The real unpinned requests received explanation400/version missing, establishing the need for the approved validation amendment. All probe processes are stopped.

## Decisions

- Dispatch `explain` before invoking the existing server parser. Rejected flattening/replacing Config: it risks changing established server flags, environment defaults and help/version behavior. Server startup stays the existing no-subcommand path.
- Mirror the captured selector pairs and `latestby=cvp`; encode using reqwest's URL support, preserving base paths. Rejected client-side selector resolution: it would introduce a separate snapshot and change the real CLI query.
- Require one selector and one target for this single-subject command. Repeated `--ignore NAME[=VERSION]` translates to captured ignore pairs. No retry or dry-run policy.
- Use existing clap and reqwest. Basic credentials require both user and password and conflict with bearer auth. Explicit options override matching environment variables. Reject invalid/non-HTTP base URLs, URL credentials and redirects; use a bounded request timeout. Diagnostics never print credentials.
- JSON writes response bytes unchanged; text prints the decision, decisive reasons, actions and exact targets/evidence links. Open reason/action codes remain printable. Allowed/blocked/unknown exits are 0/1/2; usage, transport, auth, invalid response and HTTP errors exit3. Help/version exit0.

## Ordered work and checks

1. Assertion-red main-branch200 test, explanation-only validation, corpus eligibility updates; amend accepted API plan/docs without changing Ruby routes/schema unless necessary.
2. Assertion-red CLI mapping tests against the real query recordings; implement parser, query builder, HTTP client, output and main dispatch.
3. Subprocess tests run the built binary as server and client for decisions, all selectors, auth/env/options, malformed responses and errors; add explain checks to phase4 real-client e2e.
4. Prove no-subcommand server flags/environment/help/version, Docker ENTRYPOINT and compose startup, and execute the Homebrew formula test against the candidate binary. Run fmt, clippy, all tests and e2e. Mutation-check changed guards, strengthening survivors test-first.
5. Complete non-Codex diff review, probe findings, fix verified ones test-first, push and open a PR to main. Never merge.
