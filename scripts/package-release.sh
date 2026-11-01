#!/usr/bin/env bash
# Run after cargo build --locked --release --target <target>.
set -euo pipefail
cd "$(dirname "$0")/.."
target=$1
tag=$2
version=$(python3 -c 'import tomllib; print(tomllib.load(open("Cargo.toml", "rb"))["package"]["version"])')
[ "$tag" = "v$version" ] || { echo 'tag does not match Cargo.toml' >&2; exit 1; }
mkdir -p dist
archive="pactling-$version-$target.tar.gz"
tar -czf "dist/$archive" -C "${CARGO_TARGET_DIR:-target}/$target/release" pactling
(cd dist && shasum -a 256 "$archive" >"$archive.sha256")
