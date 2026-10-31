# Shared by every e2e script: builds pactling once, starts it on a free port with a fresh
# SQLite file, exports BROKER, and stops it on exit.
set -euo pipefail
E2E_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$E2E_DIR/.." && pwd)"
PB="$E2E_DIR/node_modules/.bin/pact-broker"
[ -x "$PB" ] || { echo "run 'npm ci' in e2e/ first" >&2; exit 2; }

fail() { echo "FAIL: $*" >&2; exit 1; }

if [ -z "${PACTLING_BIN:-}" ]; then
  (cd "$ROOT" && cargo build -q)
  PACTLING_BIN="$ROOT/target/debug/pactling"
fi

E2E_TMP="$(mktemp -d)"
PORT=$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1])')
export BROKER="http://127.0.0.1:$PORT"
PACT_BROKER_PORT="$PORT" PACTLING_DATABASE_URL="sqlite://$E2E_TMP/e2e.db?mode=rwc" \
  "$PACTLING_BIN" >"$E2E_TMP/pactling.log" 2>&1 &
PACTLING_PID=$!
cleanup() { kill "$PACTLING_PID" 2>/dev/null || true; rm -rf "$E2E_TMP"; }
trap cleanup EXIT
for _ in $(seq 1 50); do
  curl -sf "$BROKER/" >/dev/null && break
  sleep 0.1
done
curl -sf "$BROKER/" >/dev/null || { cat "$E2E_TMP/pactling.log" >&2; fail "pactling did not start on $BROKER"; }
