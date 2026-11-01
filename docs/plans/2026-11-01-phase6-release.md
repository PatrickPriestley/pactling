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

## Probe results

Verified against real systems:
- Rust 1.99.0 runs on the host and in rust:1.99.0-bookworm on both Docker architectures.
- ARM64 Docker build succeeds. UID 65532 publishes via pact-cli 16.1.2, survives
  restart and a second container attached to the same named volume; the nonempty
  SQLite file is copied from /data. Docker reallocates ephemeral ports on restart,
  so the smoke test re-reads the published port.
- Docker Compose v2.39.4 accepts the file, starts the service on 9292, publishes a
  pact, and can-i-deploy exits 1 with the expected missing-verification explanation.
- macOS arm64 release build runs --version; tar creation and SHA-256 verification pass.
- Installed actionlint accepts release.yml. This is syntax validation, not hosted
  execution. The current runner-images catalog identifies macos-15 as arm64;
  macos-14 is deprecated, so the workflow uses macos-15.
- The tag mismatch test fails when the comparison guard is inverted; restored.
  Formula hash unit tests use synthetic bytes and prove only our generator's logic.
- fmt, clippy and cargo test pass. The first e2e run exposed an existing client
  parser limitation when a generated webhook UUID starts with '-'. Both separate
  and equals syntax fail in pact-cli 16.1.2. Its bundled CustomThor splits equals
  arguments. A real CLI probe with `--uuid -- -GUGBEUOSIyYtUbApOdtRQ` reaches the
  exact intended HTTP URL (404 for the absent webhook), proving the parser fix.
  The harness now places broker options before the end-of-options marker.

No application endpoint behaviour or fixture was changed. The harness fix was driven
by the failed real-client run. No new application guards were added.
- Linux x86_64 builds in the same Rust/Debian container used by release.yml under
  local Docker emulation (6 minutes). Its executable reports pactling 0.1.0. The
  workflow's archive script and checksum verification pass for both targets.
- Homebrew 7 rejects `brew install --build-from-source ./homebrew/pactling.rb` because
  formulae must belong to a tap. Created a local pactling-phase6/probe tap, copied the
  same formula with only URLs rewritten to file:// local archives, and successfully
  installed with --build-from-source. The real PatrickPriestley tap is untouched.
- The complete e2e suite passes after the CLI invocation correction, including all
  seven expected webhook deliveries. ci.yml is unchanged.

Hosted validation limits: act is not installed, so build/package/checksum commands
were executed directly in the shell and Docker, not via act. actionlint checks the
workflow structure. Runner provisioning, artifact actions, GitHub token permissions,
release upload, GHCR authentication/push, and the multi-platform registry manifest
are unverified here. No tag, release, registry image, or tap update was published.
The checked-in formula hashes identify local artifacts only; release.yml renders
and attaches a formula using its own actual artifact hashes for tap publication.
Unsigned macOS binaries were exercised locally, not through browser-download quarantine.

Final local checks: `brew test pactling-phase6/probe/pactling` passes; the x86_64
binary also passes the distroless publish/restart/new-container persistence smoke
under emulation. This runtime probe reuses the built binary; it is not a second
full x86_64 Dockerfile build. Corrupting a copy of an archive makes SHA-256 verification
fail. The final fmt/clippy/cargo-test/e2e run passes after all changes. Linux Homebrew
installation has not been tested. Probe logs are in /tmp/pactling-phase6-probe on
this machine; the local Homebrew probe tap and stopped Docker probe volumes are retained.

## Independent review and dispositions

Claude completed `agent-review code main..HEAD` with `<<AGENT-REVIEW-END>>`.
Findings were checked individually:

1. Hosted publication remains unverified, as requested. No PAT acquisition, registry
   push, throwaway release, or deletion is needed/authorized for this PR.
2. Confirmed missing license text. Added the MIT text from GitHub's licenses/mit API
   with the owner's GitHub profile name. A new archive test failed because LICENSE
   was absent, then passed after packaging it. The runtime image includes it at
   /usr/share/licenses/pactling/LICENSE; the smoke test copies and compares it.
3. QEMU speed/robustness and publication ordering are retained operational limits,
   not a reproduced failure. The local reverse-architecture build took 6 minutes.
   Hosted ARM64-under-x86 QEMU and registry publication remain unverified; no claim
   of atomic publication or rollback is made.
4. Confirmed prerelease tags would move latest. Added a tested tag generator that
   omits latest for hyphenated prerelease versions. Its inverted guard is killed by
   tests. Stable versions continue to publish both tags.
5. Packaging tests now run in the Linux release job and are listed in Develop.
   ci.yml stays unchanged per the explicit handoff requirement.
6. Dismissed the assumed compiler override: dtolnay/rust-toolchain@stable action.yml
   calls `rustup default`, never exports RUSTUP_TOOLCHAIN. Locally `rustup default`
   reports stable while `rustup show active-toolchain` reports 1.99.0 overridden by
   this repo's file. Evidence: receiver's exact source and real rustup.
7. Dismissed "builds instead of pulling": an actual Compose v2.39.4 scratch project
   with image + build tries to pull first, then builds on registry failure. README
   now explicitly uses compose pull and up --no-build for published images, avoiding
   stale images or fallback builds in that path.
8. Confirmed GitHub's documented initial private GHCR visibility. README now tells
   the maintainer to make the first package public before distributing install commands.
   Source: https://docs.github.com/en/packages/working-with-a-github-packages-registry/working-with-the-container-registry
9. brew test had completed after the pinned review snapshot; its log shows the
   installed binary's --version assertion passing. Repeated after adding LICENSE.
10. Restored the CLI helper's captured diagnostics and exit assertion by putting
    broker options immediately after the command, before the remaining arguments.
    The webhook call retains --uuid -- <uuid>; the real-client suite checks the change.

The archive test uses synthetic binary bytes to test archive contents, not executable
compatibility; both real release executables were built and run separately. Archive
creation disables macOS AppleDouble metadata. Rebuilt tarballs and their formula
hashes reflect LICENSE inclusion, and both checksum sets pass in Debian coreutils.

The second Claude review completed with its end marker and one remaining finding:
GitHub releases also need the prerelease flag. Added an explicit true/false step
output alongside the image tags and passed it to `gh release create --prerelease=`.
The extended regression assertion failed before the change and passes afterward;
inverting prerelease classification is killed by the test. The test also asserts
the GITHUB_OUTPUT delimiter layout. Installed gh 2.92.0 accepts both boolean forms;
its exact v2.92.0 create.go declares the bool flag at line 209 and places the value
in the API's `prerelease` field at line 450. This is receiver-source evidence, not a
published release. Also executed all five packaging tests inside the exact Linux
Rust/Debian image (not just the host); they pass.
