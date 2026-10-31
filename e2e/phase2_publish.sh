#!/usr/bin/env bash
# Phase 2 acceptance: a pact-js 13 consumer writes a pact, the pact-broker CLI publishes it
# with a branch and a tag, and the CLI's reads and writes behave as they do against Ruby
# broker 2.121.2 (outputs recorded in tests/fixtures/cli/phase2/).
source "$(dirname "$0")/lib.sh"
FIX="$ROOT/tests/fixtures/cli/phase2"
PACTS="$E2E_TMP/pacts"

# Compares CLI output with a recorded fixture, ignoring the broker host, timestamps and
# the recorded exit code line.
expect_output() { # <actual> <fixture file>
  local actual expected
  actual=$(sed -E 's#http://[^/ ]+/#http://HOST/#g; s/[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9:]{8}\+00:00/<ts>/g' <<<"$1")
  expected=$(sed -E '/^exit=[0-9]+$/d; s#http://[^/ ]+/#http://HOST/#g; s/[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9:]{8}\+00:00/<ts>/g' "$2")
  [ "$actual" = "$expected" ] || fail "output differs from $(basename "$2"):
--- expected
$expected
--- actual
$actual"
}

echo "pact-js consumer test writes a pact"
PACT_DIR="$PACTS" node --test "$E2E_DIR/consumer.test.js" >"$E2E_TMP/consumer.log" 2>&1 || { cat "$E2E_TMP/consumer.log"; fail "consumer test failed"; }
[ -f "$PACTS/Consumer-Provider.json" ] || fail "no pact written in $PACTS"

echo "pact-broker publish with a branch and a tag matches the Ruby broker's output"
out=$("$PB" publish "$PACTS" --consumer-app-version 1.0.0 --branch main --tag dev --broker-base-url "$BROKER" 2>&1)
expect_output "$out" "$FIX/publish-1.0.0.txt"
out=$("$PB" publish "$PACTS" --consumer-app-version 1.0.0 --branch main --tag dev --broker-base-url "$BROKER" 2>&1)
expect_output "$out" "$FIX/publish-1.0.0-again.txt"

echo "the published pact is served back with the interaction"
count=$(curl -sf "$BROKER/pacts/provider/Provider/consumer/Consumer/version/1.0.0" | python3 -c 'import json,sys; d=json.load(sys.stdin); print(len(d["interactions"]), d["consumer"]["name"], d["_links"]["pb:consumer-version"]["name"])')
[ "$count" = "1 Consumer 1.0.0" ] || fail "unexpected pact document: $count"

echo "changing the content of a published version is refused"
mkdir -p "$E2E_TMP/changed"
python3 - "$PACTS/Consumer-Provider.json" "$E2E_TMP/changed/Consumer-Provider.json" <<'PY'
import json, sys
d = json.load(open(sys.argv[1]))
d["interactions"][0]["response"]["status"] = 201
json.dump(d, open(sys.argv[2], "w"), indent=2)
PY
set +e; out=$("$PB" publish "$E2E_TMP/changed" --consumer-app-version 1.0.0 --branch main --broker-base-url "$BROKER" 2>&1); code=$?; set -e
[ "$code" = 1 ] || fail "expected exit 1 for changed content, got $code: $out"
[ "$(head -1 <<<"$out")" = "$(head -1 "$FIX/publish-1.0.0-changed.txt")" ] || fail "unexpected 409 message: $out"

echo "list-latest-pact-versions shows the integration"
out=$("$PB" list-latest-pact-versions --broker-base-url "$BROKER")
grep -Eq '^Consumer \| 1\.0\.0 +\| Provider \| [0-9]{4}-' <<<"$out" || fail "unexpected list: $out"

echo "describe-version shows the tag"
out=$("$PB" describe-version --pacticipant Consumer --version 1.0.0 --broker-base-url "$BROKER")
expect_output "$out" "$FIX/describe-version-1.0.0.txt"
out=$("$PB" describe-version --pacticipant Consumer --latest dev --broker-base-url "$BROKER")
expect_output "$out" "$FIX/describe-version-latest-dev.txt"

echo "create-version-tag, create-or-update-version and delete-branch"
out=$("$PB" create-version-tag --pacticipant Consumer --version 1.0.0 --tag qa --tag uat --broker-base-url "$BROKER")
[ "$out" = "$(sed 's/1\.0\.1/1.0.0/; /^exit=/d' "$FIX/create-version-tag-1.0.1.txt")" ] || fail "unexpected tag output: $out"
out=$("$PB" create-or-update-version --pacticipant Consumer --version 2.0.0 --branch main --tag t3 --broker-base-url "$BROKER")
[ "$out" = "$(sed 's/11\.0\.0/2.0.0/; /^exit=/d' "$FIX/create-or-update-version-11.0.0.txt")" ] || fail "unexpected create-or-update-version output: $out"
"$PB" create-or-update-version --pacticipant Consumer --version 3.0.0 --branch feat --broker-base-url "$BROKER" >/dev/null
out=$("$PB" delete-branch --pacticipant Consumer --branch feat --broker-base-url "$BROKER")
expect_output "$out" "$FIX/delete-branch-feat.txt"
set +e; out=$("$PB" delete-branch --pacticipant Consumer --branch feat --broker-base-url "$BROKER" 2>&1); code=$?; set -e
[ "$code" = 1 ] || fail "expected exit 1 deleting a missing branch, got $code"
expect_output "$out" "$FIX/delete-branch-feat-again.txt"

echo "describe-pacticipant reports the detected main branch"
out=$("$PB" describe-pacticipant --name Consumer --broker-base-url "$BROKER")
grep -q '^Main Branch: main$' <<<"$out" || fail "main branch not detected: $out"

echo "phase 2 e2e: PASS"
