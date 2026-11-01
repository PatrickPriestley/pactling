#!/usr/bin/env bash
# Phase 4 acceptance: the pact-broker CLI creates an environment, records deployments and
# asks can-i-deploy against pactling; the verdict says yes for a verified consumer, no for
# a changed pact until the provider verifies it, and yes again afterwards. Outputs are
# compared with the Ruby broker 2.121.2 recordings in tests/fixtures/cli/phase4/.
source "$(dirname "$0")/lib.sh"
FIX="$ROOT/tests/fixtures/cli/phase4"
PACTS="$E2E_TMP/pacts"

# Compares CLI output with a recorded fixture, ignoring the broker host, uuids,
# verification result numbers and the recorded exit code line.
normalise() { sed -E 's#http://[^/ ]+/#http://HOST/#g; s/[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}/UUID/g; s#verification-results/[0-9]+#verification-results/N#g; s#pact-version/[0-9a-f]{40}#pact-version/SHA#g'; }
expect_output() { # <actual> <fixture file>
  local actual expected
  actual=$(normalise <<<"$1")
  expected=$(sed -E '/^exit=[0-9]+$/d' "$2" | normalise)
  [ "$actual" = "$expected" ] || fail "output differs from $(basename "$2"):
--- expected
$expected
--- actual
$actual"
}
cli() { # <expected exit> <fixture> <args...>: runs the CLI, checks exit code and output
  local want=$1 fixture=$2; shift 2
  set +e; out=$("$PB" "$@" --broker-base-url "$BROKER" 2>&1); code=$?; set -e
  [ "$code" = "$want" ] || fail "expected exit $want from '$*', got $code: $out"
  expect_output "$out" "$FIX/$fixture"
}


# Ruby combines blocked and unknown into exit 1; explain preserves unknown as exit 2.
# Compare the real CLI status just observed with its explicit counterpart below.
explain_matches() { # <explain exit> <real CLI exit> <same query options...>
  local want=$1 real_want=$2; shift 2
  [ "$code" = "$real_want" ] || fail "unexpected preceding can-i-deploy exit: $code"
  local explained actual decision_code
  set +e; explained=$("$PACTLING_BIN" explain "$@" --broker-base-url "$BROKER" --output json); actual=$?; set -e
  [ "$actual" = "$want" ] || fail "expected explain exit $want, got $actual: $explained"
  decision_code=$(python3 -c 'import json,sys; print({"allowed":0,"blocked":1,"unknown":2}[json.load(sys.stdin)["decision"]])' <<<"$explained")
  [ "$actual" = "$decision_code" ] || fail "explain exit disagrees with its decision: $explained"
}

echo "consumer publishes 1.0.0 on main and provider 1 verifies it"
PACT_DIR="$PACTS" node --test "$E2E_DIR/consumer.test.js" >"$E2E_TMP/consumer.log" 2>&1 || { cat "$E2E_TMP/consumer.log"; fail "consumer test failed"; }
"$PB" publish "$PACTS" --consumer-app-version 1.0.0 --branch main --broker-base-url "$BROKER" >/dev/null
BROKER="$BROKER" PROVIDER_VERSION=1 PROVIDER_BRANCH=main node --test "$E2E_DIR/provider.test.js" >"$E2E_TMP/provider1.log" 2>&1 || { cat "$E2E_TMP/provider1.log"; fail "provider verification failed"; }

echo "create-environment and record-deployment match the Ruby broker's output"
cli 0 create-environment-test.txt create-environment --name test --display-name "Test" --no-production
cli 1 create-environment-duplicate.txt create-environment --name test
cli 0 create-environment-production.txt create-environment --name production --display-name "Production" --production --contact-name "Ops" --contact-email-address ops@example.com
cli 0 record-deployment-provider.txt record-deployment --pacticipant Provider --version 1 --environment test
cli 1 record-deployment-unknown-environment.txt record-deployment --pacticipant Consumer --version 1.0.0 --environment nope
cli 1 record-deployment-unknown-version.txt record-deployment --pacticipant Consumer --version 9.9.9 --environment test

echo "can-i-deploy says yes for the verified consumer version"
cli 0 can-i-deploy-yes.txt can-i-deploy --pacticipant Consumer --version 1.0.0 --to-environment test
explain_matches 0 0 --pacticipant Consumer --version 1.0.0 --to-environment test
cli 1 can-i-deploy-unknown-version.txt can-i-deploy --pacticipant Consumer --version 9.9.9 --to-environment test
explain_matches 1 1 --pacticipant Consumer --version 9.9.9 --to-environment test
cli 0 record-deployment-consumer.txt record-deployment --pacticipant Consumer --version 1.0.0 --environment test

echo "a changed consumer pact is not deployable until the provider verifies it"
mkdir -p "$E2E_TMP/changed"
python3 - "$PACTS/Consumer-Provider.json" "$E2E_TMP/changed/Consumer-Provider.json" <<'PY'
import json, sys
d = json.load(open(sys.argv[1]))
d["interactions"][0]["description"] = "a request for an order (v2)"
json.dump(d, open(sys.argv[2], "w"), indent=2)
PY
"$PB" publish "$E2E_TMP/changed" --consumer-app-version 2.0.0 --branch main --broker-base-url "$BROKER" >/dev/null
cli 1 can-i-deploy-unverified.txt can-i-deploy --pacticipant Consumer --version 2.0.0 --to-environment test
explain_matches 2 1 --pacticipant Consumer --version 2.0.0 --to-environment test

echo "a failed verification by the deployed provider keeps the answer no, unless the provider is ignored"
# The changed pact is pending for the provider's main branch, so its failure does not fail
# the verifier's build (exit 0, as against the Ruby broker); the result is still published.
BROKER="$BROKER" PROVIDER_VERSION=1 PROVIDER_BRANCH=main FAIL=true SELECTORS='[{"branch":"main"}]' node --test "$E2E_DIR/provider.test.js" >"$E2E_TMP/provider-fail.log" 2>&1 || { cat "$E2E_TMP/provider-fail.log"; fail "pending failure should not fail the build"; }
grep -q "There were 1 non-fatal pact failures on pending pacts" "$E2E_TMP/provider-fail.log" || { cat "$E2E_TMP/provider-fail.log"; fail "expected a pending failure"; }
cli 1 can-i-deploy-failed.txt can-i-deploy --pacticipant Consumer --version 2.0.0 --to-environment test
explain_matches 1 1 --pacticipant Consumer --version 2.0.0 --to-environment test
cli 0 can-i-deploy-ignore.txt can-i-deploy --pacticipant Consumer --version 2.0.0 --to-environment test --ignore Provider
explain_matches 0 0 --pacticipant Consumer --version 2.0.0 --to-environment test --ignore Provider

echo "a later successful verification flips the answer to yes"
BROKER="$BROKER" PROVIDER_VERSION=1 PROVIDER_BRANCH=main SELECTORS='[{"branch":"main"}]' node --test "$E2E_DIR/provider.test.js" >"$E2E_TMP/provider2.log" 2>&1 || { cat "$E2E_TMP/provider2.log"; fail "provider verification of 2.0.0 failed"; }
verdict=$(curl -sf "$BROKER/can-i-deploy?pacticipant=Consumer&version=2.0.0&environment=test" | python3 -c 'import json,sys; d=json.load(sys.stdin)["summary"]; print(d["deployable"], d["success"], d["failed"], d["unknown"])')
[ "$verdict" = "True 1 0 0" ] || fail "unexpected verdict after verification: $verdict"
set +e; out=$("$PB" can-i-deploy --pacticipant Consumer --version 2.0.0 --to-environment test --broker-base-url "$BROKER" 2>&1); code=$?; set -e
[ "$code" = 0 ] || fail "expected yes after verification: $out"
[ "$(head -1 <<<"$out")" = "$(head -1 "$FIX/can-i-deploy-yes.txt")" ] || fail "unexpected first line: $out"
explain_matches 0 0 --pacticipant Consumer --version 2.0.0 --to-environment test

echo "undeployment and a newer deployment are recorded"
cli 0 record-undeployment.txt record-undeployment --pacticipant Consumer --environment test
cli 1 record-undeployment-again.txt record-undeployment --pacticipant Consumer --environment test
cli 0 record-deployment-newer.txt record-deployment --pacticipant Consumer --version 2.0.0 --environment test
test_uuid=$(curl -sf "$BROKER/environments" | python3 -c 'import json,sys; print([e["uuid"] for e in json.load(sys.stdin)["_embedded"]["environments"] if e["name"]=="test"][0])')
current=$(curl -sf "$BROKER/environments/$test_uuid/deployed-versions/currently-deployed" | python3 -c 'import json,sys; print(sorted((d["_embedded"]["pacticipant"]["name"], d["_embedded"]["version"]["number"]) for d in json.load(sys.stdin)["_embedded"]["deployedVersions"]))')
[ "$current" = "[('Consumer', '2.0.0'), ('Provider', '1')]" ] || fail "unexpected currently-deployed: $current"

echo "phase 4 e2e: PASS"
