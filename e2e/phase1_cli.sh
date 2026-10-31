#!/usr/bin/env bash
# Phase 1 acceptance: the real pact-broker CLI behaves against pactling exactly as it does
# against Ruby broker 2.121.2 (outputs recorded in tests/fixtures/cli/).
source "$(dirname "$0")/lib.sh"

echo "describe-pacticipant on an empty broker exits 1 with the 404 body"
set +e; out=$("$PB" describe-pacticipant --name Nope --broker-base-url "$BROKER" 2>&1); code=$?; set -e
[ "$code" = 1 ] || fail "expected exit 1, got $code: $out"
grep -q "status=404 {\"error\":\"No pacticipant with name 'Nope' found\"}" <<<"$out" || fail "unexpected output: $out"

echo "list-latest-pact-versions prints No data."
out=$("$PB" list-latest-pact-versions --broker-base-url "$BROKER")
[ "$out" = "No data." ] || fail "unexpected output: $out"

echo "list-environments exits 0 on an empty broker"
"$PB" list-environments --broker-base-url "$BROKER" >/dev/null

echo "create-or-update-pacticipant creates, then updates"
out=$("$PB" create-or-update-pacticipant --name Bar --main-branch trunk --repository-url https://example.com/bar --broker-base-url "$BROKER")
grep -q 'Pacticipant "Bar" created in the Pact Broker' <<<"$out" || fail "unexpected output: $out"
out=$("$PB" create-or-update-pacticipant --name Bar --main-branch trunk --repository-url https://example.com/bar2 --broker-base-url "$BROKER")
grep -q 'Pacticipant "Bar" updated in the Pact Broker' <<<"$out" || fail "unexpected output: $out"

echo "describe-pacticipant prints the same fields as the Ruby broker"
out=$("$PB" describe-pacticipant --name Bar --broker-base-url "$BROKER")
expected=$(sed -E "/^(Updated|Created) At:/d" "$ROOT/tests/fixtures/cli/describe-pacticipant-Bar.txt")
actual=$(sed -E "/^(Updated|Created) At:/d" <<<"$out")
[ "$actual" = "$expected" ] || fail "describe-pacticipant differs:
--- expected
$expected
--- actual
$actual"
grep -qE "^Created At: '[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}\+00:00'$" <<<"$out" || fail "Created At format: $out"

echo "basic auth: the CLI is refused without credentials and accepted with them"
AUTH_PORT=$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1])')
PACT_BROKER_PORT="$AUTH_PORT" PACTLING_DATABASE_URL="sqlite://$E2E_TMP/auth.db?mode=rwc" \
  PACT_BROKER_BASIC_AUTH_USERNAME=rw PACT_BROKER_BASIC_AUTH_PASSWORD=rwpass \
  "$PACTLING_BIN" >"$E2E_TMP/auth.log" 2>&1 &
AUTH_PID=$!
trap 'kill $AUTH_PID 2>/dev/null || true; cleanup' EXIT
for _ in $(seq 1 50); do curl -s -o /dev/null "http://127.0.0.1:$AUTH_PORT/" && break; sleep 0.1; done
set +e; out=$("$PB" list-environments --broker-base-url "http://127.0.0.1:$AUTH_PORT" 2>&1); code=$?; set -e
[ "$code" != 0 ] || fail "expected a failure without credentials"
grep -q "status=401" <<<"$out" || fail "expected status=401 in: $out"
"$PB" list-environments --broker-base-url "http://127.0.0.1:$AUTH_PORT" --broker-username rw --broker-password rwpass >/dev/null

echo "phase 1 e2e: PASS"
