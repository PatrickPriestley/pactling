#!/usr/bin/env bash
# Account-dependent acceptance: real Codex, fresh demo broker, no prompt answering.
set -euo pipefail
: "${PACTLING_BIN:?Set PACTLING_BIN to an absolute path to a binary built with --features mcp}"
exec python3 "$(dirname "$0")/mcp_read_only.py" "$PACTLING_BIN" "${1:?Usage: mcp_read_only.sh OUTPUT_DIRECTORY}"
