# Phase 6 handoff: release packaging

Written 2026-11-01 for the agent taking over Phase 6. Read `AGENTS.md` and
`~/AGENT-WORKFLOW.md` first; their rules apply (probe before asserting, red before green,
mutation-check guards, independent review by another model family before pushing,
Conventional Commits with no AI attribution, never merge into `main` yourself).

## State at handoff (commit `edc5e4c` on `main`)

Phases 1 to 5 are complete, reviewed and merged: every broker document, the matrix and
can-i-deploy, environments, deployments, releases and webhooks replay Ruby broker 2.121.2
captures (`tests/fixtures/`), and `e2e/run.sh` drives pact-js 13, the `pact-broker` CLI
(pact-cli 16.1.2 / client 1.77) and a webhook receiver against the built binary. The
binary is configured by environment (`README.md`, "Run"); the SQLite file comes from
`PACTLING_DATABASE_URL` (default `sqlite://pactling.db?mode=rwc`).

Known constraints:

- GitHub Actions does not run on this repository (account billing block), so nothing in a
  workflow can be observed on GitHub. Prove workflows locally: `act` if installed, or run
  the same commands in a shell, and say which you did.
- Docker is available locally (the Ruby reference broker runs from
  `scripts/ruby-broker/docker-compose.yml`).

## What to do, in order (work on branch `feat/release`, PR to `main`, do not merge)

1. **`Dockerfile`** (Task 6.1): multi-stage build on the pinned Rust toolchain
   (`rust-toolchain.toml`), final image distroless or equivalent minimal base, binary at
   `/usr/local/bin/pactling`, `/data` as the volume, `PACTLING_DATABASE_URL` defaulting to
   `sqlite:///data/pactling.db?mode=rwc`, port 9292 exposed, non-root user. Probe: build
   it, run it, publish a pact with the real CLI from `e2e/node_modules/.bin/pact-broker`,
   restart the container and show the pact survives in `/data`.
2. **`docker-compose.yml`** that is a drop-in for the one in
   `pact-foundation/pact-broker-docker` / `pact-broker-infra` style setups: the service
   named `pact-broker` (or `broker`) on 9292 with the same `PACT_BROKER_*` variables the
   README lists, a named volume for `/data`, and a short comment on swapping the image.
   Probe: `docker compose up` on it, then `pact-broker publish` and `can-i-deploy`.
3. **Homebrew formula** (Task 6.2) in `homebrew/pactling.rb` installing the release
   binary (bottle-less, `url` to the GitHub release asset, `sha256`, `test do` runs
   `pactling --version`). Look for an existing tap under this account first
   (`gh repo list PatrickPriestley | grep -i homebrew`) and follow its layout; if none,
   write the formula so it can be copied into `homebrew-tap/Formula/`. Probe:
   `brew install --build-from-source ./homebrew/pactling.rb` against a local tarball if a
   release asset does not exist yet, and say so.
4. **Release workflow** (Task 6.4) `.github/workflows/release.yml`: on a `v*` tag, build
   macOS arm64 and linux x86_64 binaries, attach them as tarballs with checksums to a
   GitHub release, and push the Docker image to GHCR (`ghcr.io/patrickpriestley/pactling`)
   tagged with the version and `latest`. Keep `ci.yml` as is. The version comes from
   `Cargo.toml`; add `--version` output to the binary if `pactling --version` does not
   already print it.
5. **README** (Task 6.3): a "Run" section with the compose swap (which lines change when
   replacing the Ruby broker's compose service), the Docker and Homebrew install lines, the
   environment variable table kept current, and the sentence "verified against Ruby broker
   2.121.2 and pact-cli 16.1.2". Keep the Status and deliberate-differences paragraphs.
6. Run `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test` and
   `bash e2e/run.sh`, then `agent-review code main..HEAD --goal "..."` with a reviewer
   other than your own kind, fix verified findings test-first, push the branch and open
   the PR with what was probed and how.

Leave issues #4 to #17 alone; they are roadmap items for the maintainer to triage.
