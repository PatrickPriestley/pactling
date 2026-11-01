# Explain CLI output fixes

Keep the endpoint documents, JSON output and exit codes unchanged. Improve text-mode errors and replace Target/Evidence JSON dumps with a verdict plus one human-readable line per reason: code, involved pair or participant, and linked target action. Keep concise evidence links and open codes; do not evaluate deployment policy in the client.

Verified against the real binary: an unknown environment responds HTTP 400 with `INVALID_QUERY` and an array detail, while the baseline CLI exits 3 and prints only the status. A valid unknown decision emits multiline Target/Evidence JSON. Captures are retained in `/tmp/pactling-cli-errors-proof/baseline-probe.log`. Structured-field and fallback cases are controlled protocol tests, not claims about new broker behavior.

Use structured error code plus every string detail (including field-to-string-array validation details). Reject partial/malformed shapes as a whole and display the raw body instead, bounded to 1024 Unicode characters with an explicit truncation marker. Rejected: silently dropping details, parsing only the first item, or printing unbounded fallback HTML. JSON stdout remains byte-for-byte response data.

Derive reason summaries from existing evidence references and linked actions. Retain known provider candidates from action targets when compatibility versions are null. Rejected: dumping nested JSON, inventing pairs for selector errors, hiding advisory reasons, or independently resolving versions.

Tasks: assertion-red formatter and subprocess tests; implement isolated `src/cli/error.rs` and `src/cli/text.rs` and wire them in `src/cli.rs`; update text expectations and API docs; mutation-check each changed guard; run fmt, clippy, all tests and real-client e2e; complete non-Codex review before pushing a PR. No compatibility fixtures change.
