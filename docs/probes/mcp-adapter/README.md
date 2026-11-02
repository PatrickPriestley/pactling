# MCP adapter plan probes — 2026-11-02 UTC

**Prototype only, not the production adapter.** Rust source, its Cargo manifest/lockfile,
wire wrapper and binaries are in `/tmp/pactling-mcp-probe`; none is committed. The prototype
links this fresh clone's pactling library, starts an in-memory synthetic broker, and exposes
read-only list and explanation prototype tools that GETs it using synthetic read-only credentials. Its direct HAL
pass-through, prototype errors and partial schemas are **not** the proposed public contracts.

## Build and transport

`cargo info rmcp` reported official rmcp 3.8.0, Rust minimum 1.88. A scratch Cargo package used
`pactling = { path = "/tmp/pactling-mcp-plan" }`, exact rmcp 3.8.0 with server/macros/stdio and
Streamable-HTTP features, axum 0.8, tokio 1/full, reqwest 0.13/json and serde. With dev debug
information/incremental disabled to save scratch disk space, `cargo build` succeeded on
Rust 1.99.0. [Version/hash metadata](versions.json) and inverse dependency trees for
[rmcp](rmcp-tree.txt), [tokio](tokio-tree.txt), [axum](axum-tree.txt) record resolution.
No release-size or performance claim follows from this build.

Prototype modes: `.../target/debug/pactling-mcp-prototype broker` listened on 127.0.0.1:9495;
`http` mounted rmcp's `StreamableHttpService` with `LocalSessionManager` on axum 0.8 at
127.0.0.1:9496/mcp; no argument served stdio. The HTTP mount was an isolated unauthenticated
transport probe, not a production-auth design. JSON-RPC initialize (2025-11-25), initialized,
tools/list and tools/call returned 200/202/200/200; exact bodies are in [http-probes.json](http-probes.json).
This was a direct HTTP protocol probe, distinct from the real Codex stdio client below.

## Real-client gate

Commands (all scratch configuration; normal user configuration untouched):

```sh
CODEX_HOME=/tmp/pactling-mcp-probe/codex-home codex mcp add pactling_probe -- /tmp/pactling-mcp-probe/wire.py
CODEX_HOME=/tmp/pactling-mcp-probe/codex-home codex mcp list --json
codex -a never exec --ignore-user-config --ephemeral --skip-git-repo-check --sandbox read-only \
  -c 'mcp_servers.pactling_probe.command="/tmp/pactling-mcp-probe/wire.py"' --json \
  'MCP protocol probe only. Discover the pactling_probe tools, then call list_integrations once with {} and report the actual structured result. Do not use shell or files.'
```

`wire.py` transparently forwarded stdin/stdout to the rmcp executable and captured JSON-RPC;
it did not implement an MCP server or return fake responses. Codex used its existing login
without copying/printing credentials. No trust/login/permission prompt was answered.
The first no-annotation attempt discovered tools but refused execution under policy `never`.
Adding truthful read-only/non-destructive/idempotent/closed-world annotations allowed the
same read under the unchanged policy. A typed empty argument struct with deny_unknown_fields
and an explicit output schema were then included in the final successful client run.

The final [client exchange](client-exchange.json) records initialize, tools/list and tools/call,
with client `_meta` session/trace identifiers omitted; server results are unmodified.
Codex 0.166.0 negotiated 2025-06-18. The tool's input schema is an empty object with unknown
properties disallowed; its output schema requires schemaVersion/items/nextCursor/coverage.
The [exact final tool result](tool-result.json) contains both synthetic integrations
(ProbeConsumer→ProbeProvider and OtherConsumer→OtherProvider), `nextCursor:null`,
`coverage:"recorded_only"`, matching JSON text and structured content, and `isError:false`.
Registration is in [client-registration.json](client-registration.json). This proves one
real-client read, not the five future production tools, paging cache, or stable error contract.

## Schema, auth and pagination probes

- [Advertised-only schema result](schema-errors-advertised-only.json): extra arguments were
  accepted by a zero-parameter handler despite additionalProperties:false in metadata.
- [Typed result](schema-errors.json): after successful rebuild with `Parameters<Empty>` and
  `serde(deny_unknown_fields)`, the same extra argument returned `isError:true` with text
  ``failed to deserialize parameters: unknown field `unexpected`, there are no fields``.
  The unknown tool returned JSON-RPC -32602, `tool not found`. A missing schemars import
  initially caused a compile error; it was corrected and the successful rebuild preceded
  this typed probe. Compile failures are not contract evidence.
- [Broker probes](broker-probes.json): synthetic writer seeded a pact; read-only Basic and
  static bearer GETs succeeded, unauthenticated GET and read-only POST got empty-body 401;
  exact missing pact got 404 and an unknown environment got explanation INVALID_QUERY 400.
  Credentials were test-only; captures record auth kind, not Authorization values.
- [Paging/explanation probes](paging-explanation.json): after seeding a second pair,
  pageSize=1&pageNumber=1 still returned two integrations/four pacticipants without next links.
  An empty target environment returned a valid 200 explanation with decision unknown. No
  claim of allowed/blocked MCP prototype coverage is made; those are mandatory build tests.

The source audit confirms both-direction pair semantics, GET-only auth reuse and one broker
DecisionEvaluation/read snapshot. See the plan's exact-source references. All prototype
servers were stopped after capture. No production code, dependency files or existing fixture
bytes were changed in this plan PR.

## Review follow-up probes

- [Nested real Codex exchange](nested-client-exchange.json): same command/policy as above,
  asking for `pactling_explain_deployment` with ProbeConsumer, version "1", environment "test",
  then environment "nope". Both calls completed. Discovery includes discriminated unions and
  root-object oneOf success/error output schema. Results preserve the real v1 unknown document
  and return structured INVALID_INPUT/httpStatus 400 respectively. No production schema claim:
  prototype data is open JSON and string-length constraints are not implemented.
- [Bad type](nested-invalid-type.json): direct SDK call with numeric version returns structured
  INVALID_INPUT/isError:true. [Unit variant extra field](nested-schema-errors.json) unexpectedly
  succeeds: serde deny_unknown_fields does not enforce this case; production must explicitly
  validate nested keys before HTTP. This survivor is recorded, not treated as a passing guard.
- [URL broker probes](url-edge-probes.json): Python urllib PUT then GET against pactling 9495
  and the pinned Ruby 2.121.2 broker 9292, using percent-encoded path segments. Names include
  slash, literal percent escape, dot segments, question/hash; both brokers accepted them.
  [Actual reqwest URL/send probe](reqwest-urls.jsonl) shows slash/%/question/hash reach the
  published pact, but dot segments disappear during URL building and return 404. `nul%00`
  in the first probe is a literal string; the reqwest NUL probe is a different unseeded name
  and proves no NUL round-trip. Proposed adapter rejects controls and exact dot segments.
- [Follow-up broker documents](followup-broker.json) record HAL/plain 404s, wrong-prefix 404,
  pacticipant latestVersion.number versus absent stub, and general matrix query with three
  rows versus one under latestby=cp. These are real HTTP results, not mocks.
- [Corpus subset manifest](corpus-subset.json) fixes three canonical CLI-query-compatible
  explanation cases and explicitly lists the other 105 with reasons. All 108 still belong
  in shared evaluator/projection tests; the MCP input surface is deliberately narrower.

Ruby was started with `docker compose -p pactling-mcp-probe -f scripts/ruby-broker/docker-compose.yml up -d`.
All records use synthetic names/credentials. The probe project is stopped after capture.

Final review follow-up: [five read-only GETs and edge matrix requests](final-broker-probes.json)
all succeed with read-only Basic on pactling; same-name pairs return 200 on both brokers,
unknown names return 400. Ruby's pactling extension route returns 404. Repeated setup
returns 200 for an existing pact and 400 for duplicate environment creation; subsequent reads
use the already-created environment. [Strict schema validation](schema-validation.txt)
uses Python jsonschema Draft202012Validator on the captured tools/list schemas and both
real-client structured results. No second-client compatibility claim is made.

Final checks: [missing selector probes](missing-selectors.json) used read-only Basic: unknown
pacticipant returns 400, unknown version for an existing participant returns 200/blocked in
this setup. Neither is the explanation-route 404. [Environment forwarding](env-forwarding.json)
records synthetic values observed inside the real Codex MCP child. Used the same exec
command with `mcp_servers.pactling_probe.env_vars=["PACT_BROKER_BASE_URL","PACT_BROKER_TOKEN"]`
and the two synthetic launcher variables set; the wrapper recorded received values before
starting rmcp. This verifies name-only forwarding, not unconfigured inheritance.
