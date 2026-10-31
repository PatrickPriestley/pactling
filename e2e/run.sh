#!/usr/bin/env bash
# Runs every phase script in order; the first failure stops the run.
set -euo pipefail
cd "$(dirname "$0")"
for script in phase*_*.sh; do
  echo "=== $script"
  bash "$script"
done
echo "e2e: all phases passed"
