#!/usr/bin/env bash
# Phase 5 acceptance: the pact-broker CLI creates webhooks against pactling, test-webhook
# executes one, and publishing a changed pact fires the webhooks at a local receiver
# (e2e/receiver.js) with the Ruby broker's request shape. Outputs are compared with the
# recordings in tests/fixtures/cli/phase5/.
export PACT_BROKER_WEBHOOK_SCHEME_WHITELIST=http PACTLING_WEBHOOK_INITIAL_DELAY=0 PACTLING_WEBHOOK_DELAY_STEP=0
source "$(dirname "$0")/lib.sh"
FIX="$ROOT/tests/fixtures/cli/phase5"
PACTS="$E2E_TMP/pacts"
RPORT=$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1])')
node "$E2E_DIR/receiver.js" "$RPORT" "$E2E_TMP/receiver.jsonl" >/dev/null 2>&1 & RECEIVER=$!
trap 'kill "$RECEIVER" 2>/dev/null; cleanup' EXIT
HOOK="http://127.0.0.1:$RPORT"
sleep 0.3

# Compares CLI output with a recording, ignoring hosts, uuids, timestamps and sizes.
normalise() { sed -E 's#http://[^/" ]+#http://HOST#g; s#host\.docker\.internal:18100#RECEIVER#g; s/POST 127\.0\.0\.1/POST host.docker.internal/; s#127\.0\.0\.1:'"$RPORT"'#RECEIVER#g; s/[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}/UUID/g; s/Webhook [A-Za-z0-9_-]{22} triggered/Webhook UUID triggered/g; s/\[[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9:]{8}Z\]/[T]/g; s/content-length: [0-9]+/content-length: L/; s/user-agent: .*/user-agent: UA/; s#verification-results/[0-9]+#verification-results/N#g; s#pact-version/[0-9a-f]{40}#pact-version/SHA#g; s#/metadata/[A-Za-z0-9_=-]+#/metadata/M#g'; }
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
cli() { # <expected exit> <fixture> <args...>
  local want=$1 fixture=$2; shift 2
  set +e; out=$("$PB" "$@" --broker-base-url "$BROKER" 2>&1); code=$?; set -e
  [ "$code" = "$want" ] || fail "expected exit $want from '$*', got $code: $out"
  expect_output "$out" "$FIX/$fixture"
}
BODY='{"event":"${pactbroker.eventName}","consumer":"${pactbroker.consumerName}","provider":"${pactbroker.providerName}","consumerVersion":"${pactbroker.consumerVersionNumber}","consumerBranch":"${pactbroker.consumerVersionBranch}","consumerTags":"${pactbroker.consumerVersionTags}","providerVersion":"${pactbroker.providerVersionNumber}","providerBranch":"${pactbroker.providerVersionBranch}","providerTags":"${pactbroker.providerVersionTags}","providerVersionDescriptions":"${pactbroker.providerVersionDescriptions}","pactUrl":"${pactbroker.pactUrl}","verificationResultUrl":"${pactbroker.verificationResultUrl}","github":"${pactbroker.githubVerificationStatus}","bitbucket":"${pactbroker.bitbucketVerificationStatus}","azure":"${pactbroker.azureDevOpsVerificationStatus}","gitlab":"${pactbroker.gitlabVerificationStatus}","buildUrl":"${pactbroker.buildUrl}","consumerLabels":"${pactbroker.consumerLabels}","providerLabels":"${pactbroker.providerLabels}","deployedProvider":"${pactbroker.currentlyDeployedProviderVersionNumber}"}'

echo "consumer publishes 1.0.0 (tag dev) and provider 1 verifies it; provider 2 is the main branch head"
PACT_DIR="$PACTS" node --test "$E2E_DIR/consumer.test.js" >"$E2E_TMP/consumer.log" 2>&1 || { cat "$E2E_TMP/consumer.log"; fail "consumer test failed"; }
"$PB" publish "$PACTS" --consumer-app-version 1.0.0 --branch main --tag dev --broker-base-url "$BROKER" >/dev/null
BROKER="$BROKER" PROVIDER_VERSION=1 PROVIDER_BRANCH=main node --test "$E2E_DIR/provider.test.js" >"$E2E_TMP/provider1.log" 2>&1 || { cat "$E2E_TMP/provider1.log"; fail "provider verification failed"; }
"$PB" create-environment --name test --no-production --broker-base-url "$BROKER" >/dev/null
"$PB" record-deployment --pacticipant Provider --version 1 --environment test --broker-base-url "$BROKER" >/dev/null
"$PB" create-or-update-version --pacticipant Provider --version 2 --branch main --broker-base-url "$BROKER" >/dev/null

echo "create-webhook, create-or-update-webhook and the validation errors match the Ruby broker's output"
cli 0 create-webhook-consumer-provider.txt create-webhook "$HOOK/hook/\${pactbroker.consumerName}" -X POST -H "Content-Type: application/json" -H "Authorization: Bearer hook-secret" -d "$BODY" --consumer Consumer --provider Provider --description "Trigger Provider build" --contract-content-changed
cli 0 create-webhook-provider-all-events.txt create-webhook "$HOOK/hook/all" -X POST -H "Content-Type: application/json" -d "$BODY" --provider Provider --contract-published --contract-content-changed --provider-verification-published --provider-verification-succeeded --provider-verification-failed --contract-requiring-verification-published
cli 1 create-webhook-bad-method.txt create-webhook "$HOOK/hook/get" -X GET --contract-content-changed
cli 1 create-webhook-https.txt create-webhook "https://example.com/hook" -X POST --contract-content-changed
cli 1 create-webhook-unknown-consumer.txt create-webhook "$HOOK/hook/x" -X POST --consumer Nope --contract-content-changed
cli 0 create-or-update-webhook-create.txt create-or-update-webhook "$HOOK/hook/upsert" --uuid pactling-e2e-webhook-0001 -X POST -d '{"event":"${pactbroker.eventName}"}' --consumer Consumer --provider Provider --contract-published
cli 0 create-or-update-webhook-update.txt create-or-update-webhook "$HOOK/hook/upsert" --uuid pactling-e2e-webhook-0001 -X POST -d '{"event":"${pactbroker.eventName}"}' --consumer Consumer --provider Provider --contract-published --description "Updated"

curl -sf -o /dev/null -X POST -H 'content-type: application/json' -d '{"request":{"method":"POST","url":"'"$HOOK"'/hook/global","username":"hookuser","password":"hookpass","headers":{"X-Token":"abc","Accept":"application/json"},"body":{"template":"${pactbroker.consumerVersionNumber}"}}}' "$BROKER/webhooks" || fail "could not create the global webhook"

echo "test-webhook executes the webhook against the latest pact and prints the logs"
uuid=$(curl -sf "$BROKER/webhooks" | python3 -c 'import json,sys; print(json.load(sys.stdin)["_links"]["pb:webhooks"][0]["href"].rsplit("/",1)[1])')
# End option parsing before the UUID: the real CLI rejects leading '-' even with '='.
out=$("$PB" test-webhook --broker-base-url "$BROKER" --uuid -- "$uuid")
expect_output "$out" "$FIX/test-webhook.txt"
received=$(python3 -c 'import json; r=[json.loads(l) for l in open("'"$E2E_TMP"'/receiver.jsonl")]; print(len(r), r[0]["path"], json.loads(r[0]["body"])["event"], json.loads(r[0]["body"])["consumerVersion"], r[0]["headers"]["authorization"])')
[ "$received" = "1 /hook/Consumer test 1.0.0 Bearer hook-secret" ] || fail "unexpected test execution at the receiver: $received"

echo "publishing changed content fires every matching webhook and lists them in the output"
mkdir -p "$E2E_TMP/changed"
python3 - "$PACTS/Consumer-Provider.json" "$E2E_TMP/changed/Consumer-Provider.json" <<'PY'
import json, sys
d = json.load(open(sys.argv[1]))
d["interactions"][0]["response"]["body"]["status"] = "shipped"
json.dump(d, open(sys.argv[2], "w"), indent=2)
PY
cli 0 publish-changed-3.0.0.txt publish "$E2E_TMP/changed" --consumer-app-version 3.0.0 --branch main --tag dev
for _ in $(seq 1 100); do [ "$(wc -l < "$E2E_TMP/receiver.jsonl" | tr -d ' ')" -ge 8 ] && break; sleep 0.1; done
python3 - "$E2E_TMP/receiver.jsonl" "$FIX/receiver.jsonl" <<'PY' || fail "the receiver did not get what the Ruby broker's receiver got"
import json, re, sys
got = [json.loads(l) for l in open(sys.argv[1])][1:8]
want = [json.loads(l) for l in open(sys.argv[2])][4:11]
norm = lambda s: re.sub(r"pact-version/[0-9a-f]{40}", "pact-version/SHA", re.sub(r"verification-results/\d+", "verification-results/N", re.sub(r"http://[^/\"]+", "http://HOST", s)))
for g, w in zip(got, want):
    assert g["method"] == w["method"] and g["path"] == w["path"], (g["path"], w["path"])
    assert norm(g["body"]) == norm(w["body"]), (g["body"], w["body"])
    for h in ("content-type", "authorization", "accept"):
        assert g["headers"].get(h) == w["headers"].get(h), (h, g["headers"].get(h), w["headers"].get(h))
print("7 deliveries match")
PY
status=$(curl -sf "$BROKER/pacts/provider/Provider/consumer/Consumer/webhooks/status" | python3 -c 'import json,sys; d=json.load(sys.stdin); print(d["summary"])')
[ "$status" = "{'successful': 6, 'failed': 0}" ] || fail "unexpected webhook status: $status"

echo "phase 5 e2e: PASS"
