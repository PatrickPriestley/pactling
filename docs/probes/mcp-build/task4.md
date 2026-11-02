# MCP build task 4: paging and parity

Base: `73deb70`; branch `feat/mcp-task4`.

Before implementation: replace only the two expressly transitional assertions in
`tests/mcp_catalog_transitional.rs`: a multi-page catalog now succeeds with a cursor,
and malformed `previous-process` is CURSOR_INVALID (a syntactically valid absent UUID
is CURSOR_EXPIRED). Existing mixed-field precedence assertions stay unchanged.
All other prior tests and fixture bytes remain protected.

Implementation decision: calculate every page boundary and random UUID token once
when freezing the response. Serialize the complete entry (items, tool, source,
retrievedAt, original pageSize, boundaries and tokens) for the cache budget.
A mutex serializes insertion/lookup, so concurrent replay reads the same immutable
boundaries and tokens. The monotonic creation instant controls five-minute TTL and
insertion order controls eviction; reads never refresh either.

Cancellation evidence (supported by receiver exact source): rmcp 3.8.0
`src/service.rs:1634-1638` removes the request cancellation token and cancels it on
notifications/cancelled. RequestContext.ct is public at 1195-1196. Handler tasks
are independently spawned, so the adapter must explicitly select cancellation
against its HTTP/dispatch future to drop that future and its response resources.
Real stdio cancellation verification is required below.
