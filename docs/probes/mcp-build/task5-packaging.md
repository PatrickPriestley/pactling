# Task 5 packaging verification

The maintainer accepted explicit MCP inclusion in published artifacts; source Cargo defaults
remain empty. Native release jobs and the Docker release build now pass `--features mcp`.
The Docker context admits only `docs/api/mcp/*.schema.json` from documentation, and the
build stage copies those embedded discovery/runtime schemas. No runtime documentation or
sidecar dependency is added to the final distroless image.

## Red, green and mutation checks

**Verified by packaging contract tests:** `python3 scripts/test-release.py` initially failed
three assertions: native release feature flag, image feature flag, and schema context.
The committed red test (`a06c2f9`) is unchanged by the green implementation (`c859aaa`).
The complete packaging suite now passes all eight tests.

```text
........
----------------------------------------------------------------------
Ran 8 tests in 0.165s

OK
```

Eight independent mutations each failed a test assertion and were restored immediately:
remove the macOS release flag; remove the Linux release flag; remove the Docker flag;
remove Docker schema COPY; remove each of the four context allowlist entries. No mutant
remains. Logs: `/tmp/pactling-mcp-packaging-red.log`,
`/tmp/pactling-mcp-packaging-mutations.json`, and
`/tmp/pactling-mcp-packaging-mutation-*.log`.

## Dependency overhead

**Verified against Cargo 1.99.0's resolved normal dependency graph**, using the locked
source tree, `cargo tree --locked --edges normal --prefix none --format '{p}'`, repeated
with `--features mcp`, then deduplicating package/version lines and removing tree markers:

| Configuration | Unique normal packages, including pactling |
| --- | ---: |
| Source default | 199 |
| MCP | 251 |
| Added | 52 |

The source default count matches the independently captured baseline. Full graphs and
sorted package lists: `/tmp/pactling-mcp-packaging-proof/{default,mcp}-{tree,dependencies}.txt`.
This is dependency-count evidence, not a runtime memory or performance benchmark.

## Actual native release and Homebrew proof

**Verified against actual built macOS arm64 binaries**, source commit `c859aaa`, Rust
1.99.0, host `aarch64-apple-darwin`. Both builds use identical release settings except the
feature flag; no debug/strip profile override is applied:

```sh
CARGO_TARGET_DIR=/tmp/pactling-mcp-probe/target CARGO_INCREMENTAL=0 cargo build --locked --release -j2
# Preserve target/release/pactling as pactling-default before the next build.
CARGO_TARGET_DIR=/tmp/pactling-mcp-probe/target CARGO_INCREMENTAL=0 cargo build --locked --release --features mcp -j2
```

```text
Default: Finished `release` profile [optimized] target(s) in 2m 33s
MCP:     Finished `release` profile [optimized] target(s) in 2m 08s
```

| Binary | Bytes |
| --- | ---: |
| Source default | 19,169,360 |
| MCP release | 25,371,424 |
| Increase | 6,202,064 (32.35%) |

These are same-host binary sizes, not cross-platform or memory benchmarks. The second
build reuses dependencies, so the elapsed times are not comparative compilation benchmarks.
Both binaries pass version/help, no-subcommand server startup, environment port/file-database
configuration, and HTTP `/integrations` status 200. Source default `mcp` exits 3, stdout empty,
stderr `MCP support is not enabled; rebuild with --features mcp`. The MCP release initializes
protocol `2025-06-18`, lists exactly five tools, and returns structured `BROKER_UNAVAILABLE`
for a real refused connection. Its stderr remains empty. This packaging protocol smoke is
supplementary; the mandatory real Codex all-five-tools proof is recorded separately.

The real MCP executable was copied into a disposable target directory and packaged with:

```sh
CARGO_TARGET_DIR=/tmp/pactling-mcp-packaging-proof/staged \
  bash scripts/package-release.sh aarch64-apple-darwin v0.1.1
(cd dist && shasum -a 256 -c pactling-0.1.1-aarch64-apple-darwin.tar.gz.sha256)
```

The archive contains the 25,371,424-byte executable and 1,075-byte LICENSE; checksum passes.
**Verified against Homebrew 7.0.11** using a temporary `pactling-mcp-probe/local` tap and
formula named `pactling-mcp-probe`, local archive URL/hash, and the release formula's same
`bin.install "pactling"` and version-test definitions. No remote release was downloaded or
published. Homebrew rejects a formula outside a tap, so the initial direct-path attempt is
not claimed as successful. `brew install --formula --skip-link --yes` succeeds. The Cellar
binary passes the same server/MCP smokes. `brew test` refuses an unlinked formula; after
verifying `/opt/homebrew/bin/pactling` absent, temporarily linking this probe allows the
actual formula test to pass. The probe is then unlinked, uninstalled and untapped.

Raw commands/helpers/results are under `/tmp/pactling-mcp-packaging-proof/`: `smoke.py`,
`mcp-smoke.py`, `native-*-build.log`, `native-*-smoke.json`, `native-mcp-protocol.json`,
`native-default-mcp-disabled.json`, `binary-size.json`, `brew-*.log`, `brew-*-smoke.json`.
Only macOS arm64 Homebrew installation is proven; other release architectures and hosted
GitHub Actions execution are not inferred from this local test.

## Docker/compose limitation and recovery record

**Superseded by the verification below.** At the time of this attempt: actual Docker image
build, container runtime and compose smoke were not verified. The
exact updated Dockerfile was submitted for `linux/arm64` using:

```sh
docker buildx create --name pactling-mcp-packaging --driver docker-container
docker buildx build --builder pactling-mcp-packaging --platform linux/arm64 \
  --load -t pactling-mcp-proof:local --progress plain .
```

Using an isolated builder to protect existing caches was a mistaken resource choice:
it duplicated the large Rust base in its own volume. The host ran out of space before
compilation, and the build failed at `WORKDIR /build`, not at source compilation:

```text
#9 ERROR: commit failed: input/output error
ERROR: failed to build: failed to solve: Internal: failed to compute cache key:
commit failed: input/output error
```

Generated native release cache had already been removed after preserving both executable
snapshots. After the failure, the confirmed-idle shared generated debug cache was removed
with supervisor authorization; stable debug/baseline binaries, logs and all source worktrees
were preserved. This freed approximately 1.4 GiB, restoring 1.6 GiB host free space. No
existing Docker image, volume or cache was pruned. The attempted fallback using the default
builder never started (the shell could not create its log while the disk was full).

Removing the probe's own container and volume failed because Docker's metadata writes
returned `input/output error`. An ordinary `docker desktop restart` also failed to return
within several minutes; Docker VM logs continued reporting containerd metadata/blob I/O
errors. The hanging probe CLI processes were terminated; no cargo/build process remains.
No destructive reset or manual modification of Docker's owner data was attempted.

Pending cleanup when Docker is healthy (only resources created by this probe):

```sh
docker rm -f buildx_buildkit_pactling-mcp-packaging0
docker volume rm buildx_buildkit_pactling-mcp-packaging0_state
```

Container ID observed: `dc1e5e62a05b9ce5470199a7a23941c5b24dd87123ce22f0cd9f4501e38ab4cd`.
The builder registration was removed before the daemon failure, but its container/volume
could not be removed. `moby/buildkit:buildx-stable-1` was downloaded by this attempt; it was
not present in the initial image inventory. No `pactling-mcp-proof:local` image was produced.
Raw build/restart/cleanup logs remain in `/tmp/pactling-mcp-packaging-proof/`. A successful
Docker/compose check remains required before claiming packaging acceptance across all
published artifact types; static contract tests alone do not establish it.

## Docker/compose verification after recovery (2026-11-02)

**Verified against Docker Desktop 28.4.0** after the host disk was freed (52 GiB free) and
Docker Desktop was restarted. Source commit `45c4cf1`. The probe's leftover builder container
and `_state` volume were removed first. The default builder was used, so no Rust base was
duplicated:

```sh
docker build --platform linux/arm64 -t pactling-mcp-proof:local --progress plain .
```

The build passed. The image is `linux/arm64`, 19,586,744 bytes, and `--version` prints
`pactling 0.1.2`. A stdio JSON-RPC driver (initialize, `notifications/initialized`,
`tools/list`, `tools/call`) checked these cases. Only throwaway names were used, and all were
removed afterwards:

| Check | Result |
| --- | --- |
| Broker container on a user network, `GET /integrations` | 200; `PUT` of a seeded pact 201 |
| `docker run -i --rm … pactling-mcp-proof:local mcp` with `PACT_BROKER_BASE_URL=http://<broker>:9292` | protocol `2025-06-18` (and `2025-11-25` in a second session), exactly five tools, seeded pair listed, exact pact returned, missing version `NOT_FOUND`, exit 0, stderr empty |
| Same command, unreachable broker host | `BROKER_UNAVAILABLE`, stderr empty |
| Repository `docker-compose.yml` with only `image:` and the host port overridden, `up -d --no-build` | `/integrations` 200, pact `PUT` 201 |
| `docker compose exec -T -e PACT_BROKER_BASE_URL=http://localhost:9292 pact-broker /usr/local/bin/pactling mcp` | five tools, seeded pair, exact pact, `NOT_FOUND`, exit 0, stderr empty |
| `docker compose restart`, then `/integrations` | seeded integration still present (named volume persists) |
| Paging with two integrations, `pageSize:1` | page 1 has a cursor; page 2 has `nextCursor:null`; replay returns the same page |
| Cursor plus `pageSize` / cross-tool cursor / malformed cursor / unknown UUID cursor | `INVALID_INPUT` `/cursor` / `CURSOR_INVALID` / `CURSOR_INVALID` / `CURSOR_EXPIRED` |
| `consumer:".."` / `consumer:"a/b"` | `INVALID_INPUT` without a GET / encoded segment, broker `NOT_FOUND` |

Still not proven: the x86_64 Linux release archive, built only by the release workflow, and
hosted release Actions. This run used a direct protocol driver, not a real MCP client inside
the container. The real Codex client check ran against the native binary.
