#!/usr/bin/env bash
# Phase 3 acceptance: a pact-js 13 provider fetches pacts with consumer version selectors,
# verifies them and publishes results against pactling; the broker's pending state flips
# once a result is published, and a failing provider publishes a failure.
source "$(dirname "$0")/lib.sh"
PACTS="$E2E_TMP/pacts"

echo "consumer publishes a pact on main"
PACT_DIR="$PACTS" node --test "$E2E_DIR/consumer.test.js" >"$E2E_TMP/consumer.log" 2>&1 || { cat "$E2E_TMP/consumer.log"; fail "consumer test failed"; }
"$PB" publish "$PACTS" --consumer-app-version 1.0.0 --branch main --broker-base-url "$BROKER" >/dev/null

echo "before any verification the pact is pending for the provider's main branch"
pending=$(curl -sf -X POST -H 'content-type: application/json' -d '{"includePendingStatus":true,"consumerVersionSelectors":[{"mainBranch":true}],"providerVersionBranch":"main"}' \
  "$BROKER/pacts/provider/Provider/for-verification" | python3 -c 'import json,sys; d=json.load(sys.stdin); p=d["_embedded"]["pacts"]; print(len(p), p[0]["verificationProperties"]["pending"], p[0]["shortDescription"])')
[ "$pending" = "1 True latest from main branch" ] || fail "unexpected for-verification: $pending"

echo "pact-js provider verifies and publishes results"
BROKER="$BROKER" PROVIDER_VERSION=1 PROVIDER_BRANCH=main node --test "$E2E_DIR/provider.test.js" >"$E2E_TMP/provider1.log" 2>&1 || { cat "$E2E_TMP/provider1.log"; fail "provider verification failed"; }
latest=$(curl -sf "$BROKER/pacts/provider/Provider/consumer/Consumer/version/1.0.0/verification-results/latest" | python3 -c 'import json,sys; d=json.load(sys.stdin); print(d["providerApplicationVersion"], d["success"], d["verifiedBy"]["client_implementation"], len(d["testResults"]))')
[ "$latest" = "1 True pact-core 1" ] || fail "unexpected latest result: $latest"
branch=$(curl -sf "$BROKER/pacticipants/Provider/versions/1" | python3 -c 'import json,sys; print([b["name"] for b in json.load(sys.stdin)["_embedded"]["branchVersions"]])')
[ "$branch" = "['main']" ] || fail "provider version 1 should be on main: $branch"

echo "after a successful verification the pact is no longer pending"
pending=$(curl -sf -X POST -H 'content-type: application/json' -d '{"includePendingStatus":true,"consumerVersionSelectors":[{"mainBranch":true}],"providerVersionBranch":"main"}' \
  "$BROKER/pacts/provider/Provider/for-verification" | python3 -c 'import json,sys; d=json.load(sys.stdin); p=d["_embedded"]["pacts"][0]["verificationProperties"]; print(p["pending"], p["notices"][1]["text"][:60])')
[ "$pending" = "False This pact has previously been successfully verified by a ver" ] || fail "unexpected pending state: $pending"

echo "a failing provider build publishes a failure and exits non-zero"
set +e; BROKER="$BROKER" PROVIDER_VERSION=2 PROVIDER_BRANCH=main FAIL=true node --test "$E2E_DIR/provider.test.js" >"$E2E_TMP/provider2.log" 2>&1; code=$?; set -e
[ "$code" != 0 ] || fail "expected the failing verification to exit non-zero"
latest=$(curl -sf "$BROKER/pacts/provider/Provider/consumer/Consumer/version/1.0.0/verification-results/latest" | python3 -c 'import json,sys; d=json.load(sys.stdin); print(d["providerApplicationVersion"], d["success"], d["testResults"][0]["mismatches"][0]["description"])')
[ "$latest" = "2 False expected 200 but was 500" ] || fail "unexpected failed result: $latest"
summary=$(curl -sf "$BROKER/verification-results/consumer/Consumer/version/1.0.0/latest" | python3 -c 'import json,sys; d=json.load(sys.stdin); print(d["success"], d["providerSummary"])')
[ "$summary" = "False {'successful': [], 'failed': ['Provider'], 'unknown': []}" ] || fail "unexpected summary: $summary"

echo "phase 3 e2e: PASS"
