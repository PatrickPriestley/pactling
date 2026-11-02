# Production adapter: real Codex acceptance

**Verified against real systems**, 2026-11-02 UTC: Codex CLI 0.166.0, the production
`pactling mcp` executable built from `459db7a`, and a real file-backed pactling HTTP broker.
No mock MCP server or mock broker was substituted. The frame helper forwards the original
stdio bytes; it does not implement initialization, discovery or tool dispatch.

Reproduce from the repository root with an already authenticated Codex client:

```sh
cargo build --features mcp
PACTLING_BIN="$PWD/target/debug/pactling" bash e2e/mcp_read_only.sh /tmp/mcp-evidence-new
```

The actual run used the supervising agent's preserved feature-enabled executable:

```sh
PACTLING_BIN=/tmp/pactling-mcp-build-proof/pactling-mcp-debug \
  bash e2e/mcp_read_only.sh /tmp/pactling-mcp-real-client-2
```

The harness registers the command using `CODEX_HOME=<temporary directory> codex mcp add`
and adds name-only `env_vars` forwarding in that temporary config. It then invokes
`codex -a never exec --ignore-user-config --ephemeral --skip-git-repo-check --sandbox read-only`
with the same command and environment-variable names. This preserves the client's existing
authentication without copying/reading credentials or changing normal MCP configuration.
No permission, trust, login or update prompt was answered.

Exact argv arrays, prompts, registration output/configs, real-client event streams and
transparent MCP frame streams are committed here. Their absolute `/tmp` paths record the
actual execution and are not portable commands; the harness reconstructs them on each run.
`http-responses.json` contains responses consumed by the adapter, including each explanation's
actual evaluation identity/time. We compare these documents directly, rather than rerunning
an explanation and ignoring volatile fields. HTTP authorization headers were never logged.
All broker credentials were synthetic disposable demo values. Frame/event output contains
no credential values. The oversized pact's padding is omitted from committed HTTP evidence;
its canonical JSON size and SHA-256 are retained.

The seed uses the committed Ruby matrix-regression capture's HTTP setup operations against
the real pactling broker. It adds a 600 KiB pact metadata field through the public publishing
route to exceed the 1 MiB MCP result budget (the result contains structured and text copies).
An earlier 5 MiB publish probe was rejected/reset by the broker's request-body bound before
any client run, so it was corrected to the bounded 600 KiB input. This is a setup correction,
not a change to production code or a claimed failing implementation test.

Literal successful harness output:

```text
"codexVersion": "codex-cli 0.166.0"
"databaseBeforeSha256": "b1a3c8e5fceaf3287b72b0c748e7601a0a014067b046d4bd34b639916a3a6ca3"
"databaseAfterSha256": "b1a3c8e5fceaf3287b72b0c748e7601a0a014067b046d4bd34b639916a3a6ca3"
"databaseAndWebhookStateUnchanged": true
"readOnlyWriteStatus": 401
"calls": 12
"exactDocumentComparisons": 5
"decisions": ["allowed", "blocked", "unknown"]
"errorCodes": ["AUTH_REQUIRED", "BROKER_UNAVAILABLE", "INVALID_INPUT", "NOT_FOUND", "RESPONSE_TOO_LARGE"]
```

The main client made twelve calls: both paged catalogs and both continuations, one exact
pact, one grouped matrix, three deployment decisions, invalid environment, missing pact,
and oversized pact. Two additional real Codex sessions produced wrong-auth and unavailable
errors. All fourteen returned structured content plus identical JSON text. Independent
post-run validation with Python jsonschema Draft202012Validator passed all fourteen against
the committed output schemas. Four returned catalog items matched source order and fields;
that comparison is also in the final reproducible harness. These post-run checks are recorded
in `summary.json` alongside the original run summary.

The complete logical SQLite dump before/after was identical, including all webhook tables;
file checksums were deliberately avoided because SQLite WAL/checkpoint housekeeping can
change files without changing broker state. Read-only credentials were also proved to reject
a POST with 401. The proxy observed GET traffic only during adapter use.

The final real-client answer reported `allowed / ALL_REQUIRED_VERIFICATIONS_SUCCESSFUL`,
`blocked / VERIFICATION_FAILED` and `unknown / VERIFICATION_MISSING`, and explicitly said:
“Results describe **recorded evidence, not live health**; explanations reported currentness
`not_checked`.” See `read-only-client.jsonl` for the full actual output and
`read-only-frames.jsonl` for protocol evidence. Discovery advertised only the five typed
read tools, with all four expected annotations; the client negotiated 2025-06-18.

This validates the recorded production binary/client combination. It does not claim all MCP
clients, all Codex versions, arbitrary production datasets, or GitHub Actions/account access.
Deterministic protocol tests separately cover 2025-06-18 and 2025-11-25. No release was
published and no image was pushed by this check.
