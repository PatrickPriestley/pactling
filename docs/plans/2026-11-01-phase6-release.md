# Phase 6 release packaging

Goal: deliver container, Compose, binary archives and a copyable Homebrew formula;
open a PR without publishing a release or changing the existing tap.

Evidence so far (real systems): Docker is Linux arm64; local rustc is 1.99.0;
GitHub reports no releases and the existing tap has Formula/pomo-tui.rb. The tap
uses ordinary Formula classes, GitHub URLs, SHA-256 and an install/test pair.
Config already enables clap's package-version output; no source change needed.

Decisions: pin Rust to 1.99.0 instead of moving stable; build in Debian 12 and run
in distroless cc Debian 12 as UID 65532 with an owned /data directory. Use a named
volume rather than a host bind mount so a fresh volume inherits ownership. Keep
Compose's broker service and Ruby-compatible configuration; SQLite replaces the
Postgres service, it does not migrate its data. Release binaries target macOS arm64
and Linux x86_64 GNU (Debian 12 baseline). Generate the release formula from the
actual archive checksums rather than inventing checksums for unpublished assets.
The checked-in formula will use checksums from local probes, and must be regenerated
by release packaging before distribution in the tap. No tap publication in this PR.

Ordered work and verification:
1. Dockerfile, .dockerignore, pinned toolchain; container smoke test publishes with
   the real CLI, recreates container with its volume, and reads the saved pact.
2. docker-compose.yml; same publish/read probe and can-i-deploy verdict.
3. homebrew/pactling.rb; install a local archive with Homebrew, run brew test.
4. release.yml and packaging scripts; execute build/archive/checksum commands locally
   for both targets; validate YAML and guards; leave ci.yml unchanged.
5. README install/swap instructions and evidence limits.
6. fmt, clippy, test, e2e; independent non-Codex review, probe findings, push and PR.

GitHub Actions billing prevents hosted execution. Local shell/Docker runs can prove
builds and packaging, but not GitHub token permissions, release upload or GHCR push.
