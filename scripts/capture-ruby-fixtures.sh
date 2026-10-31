#!/usr/bin/env bash
# Recreates every fixture under tests/fixtures/ruby-broker/<version>/ and tests/fixtures/cli/
# from a fresh Ruby Pact broker, so the evidence pactling is tested against can be
# regenerated and diffed. Needs Docker, curl, python3 and e2e/node_modules (npm ci in e2e/).
#
#   scripts/capture-ruby-fixtures.sh            # writes into tests/fixtures
#   scripts/capture-ruby-fixtures.sh /tmp/out   # writes elsewhere, for scripts/compare-fixtures.py
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$ROOT/tests/fixtures}"
COMPOSE=(docker compose -p pactling-probe -f "$ROOT/scripts/ruby-broker/docker-compose.yml")
B=http://localhost:9292
PB="$ROOT/e2e/node_modules/.bin/pact-broker"
[ -x "$PB" ] || { echo "run 'npm ci' in e2e/ first" >&2; exit 2; }

broker_up() { # restarts the broker with the current PACT_BROKER_* environment
  "${COMPOSE[@]}" up -d --force-recreate pact-broker >/dev/null 2>&1
  for _ in $(seq 1 90); do
    [ "$(curl -s -o /dev/null -w '%{http_code}' $B/)" != "000" ] && return 0
    sleep 1
  done
  echo "Ruby broker did not start" >&2; exit 1
}

echo "resetting the Ruby broker"
"${COMPOSE[@]}" down -v >/dev/null 2>&1 || true
unset PACT_BROKER_BASIC_AUTH_USERNAME PACT_BROKER_BASIC_AUTH_PASSWORD \
      PACT_BROKER_BASIC_AUTH_READ_ONLY_USERNAME PACT_BROKER_BASIC_AUTH_READ_ONLY_PASSWORD PACT_BROKER_ALLOW_PUBLIC_READ
"${COMPOSE[@]}" up -d >/dev/null 2>&1
for _ in $(seq 1 90); do curl -sf $B/ >/dev/null && break; sleep 1; done
VERSION=$(curl -s -D - -o /dev/null $B/ | tr -d '\r' | awk -F': ' 'tolower($1)=="x-pact-broker-version"{print $2}')
[ -n "$VERSION" ] || { echo "could not read x-pact-broker-version" >&2; exit 1; }
EXPECTED_VERSION="${EXPECTED_BROKER_VERSION:-2.121.2}"
[ "$VERSION" = "$EXPECTED_VERSION" ] || {
  echo "Ruby broker is $VERSION but fixtures are for $EXPECTED_VERSION; update the image digest" \
       "in scripts/ruby-broker/docker-compose.yml and EXPECTED_BROKER_VERSION together" >&2
  exit 1
}
F="$OUT/ruby-broker/$VERSION"; A="$F/auth"; C="$OUT/cli"
mkdir -p "$F" "$A" "$C"
echo "Ruby broker version $VERSION, writing to $F"

# Pretty-prints JSON from stdin, or passes non-JSON through unchanged.
pretty() { python3 -I -c 'import json,sys
raw=sys.stdin.read()
try: print(json.dumps(json.loads(raw), indent=4))
except ValueError: sys.stdout.write(raw)'; }

# GET <path> as an unauthenticated client would, saving the pretty body and the headers.
get_fixture() {
  local p=$1 f
  f="GET_$(echo "${p:-index}" | tr '/?=&' '____').json"
  curl -s -D "$F/$f.headers" -H 'Accept: application/hal+json' "$B/$p" | pretty > "$F/$f"
}
# A named request under auth/, saving the raw body and the headers.
auth_capture() { local n=$1; shift; curl -s -D "$A/$n.headers" -o "$A/$n.body" "$@"; }

echo "empty-state documents"
for p in "" pacticipants environments webhooks integrations "pacts/latest" matrix can-i-deploy \
         "pacticipants/Nope" "pacticipants/Nope/versions/1" "environments/nope" "webhooks/nope" \
         "pacts/provider/P/consumer/C/latest"; do get_fixture "$p"; done

echo "basic auth, public read off"
export PACT_BROKER_BASIC_AUTH_USERNAME=rw PACT_BROKER_BASIC_AUTH_PASSWORD=rwpass \
       PACT_BROKER_BASIC_AUTH_READ_ONLY_USERNAME=ro PACT_BROKER_BASIC_AUTH_READ_ONLY_PASSWORD=ropass \
       PACT_BROKER_ALLOW_PUBLIC_READ=false
broker_up
RW=(-u rw:rwpass); RO=(-u ro:ropass); J=(-H 'content-type: application/json'); MP=(-H 'content-type: application/merge-patch+json')
auth_capture no-creds $B/
auth_capture bad-creds -u rw:wrong $B/
auth_capture ro-get "${RO[@]}" $B/
auth_capture ro-put "${RO[@]}" -X PUT "${J[@]}" -d '{"mainBranch":"main"}' $B/pacticipants/X
auth_capture ro-post-contracts "${RO[@]}" -X POST "${J[@]}" -d '{}' $B/contracts/publish
auth_capture rw-get "${RW[@]}" $B/
auth_capture rw-put "${RW[@]}" -X PUT "${J[@]}" -d '{"repositoryUrl":"https://example.com/foo","mainBranch":"main"}' $B/pacticipants/Foo
auth_capture rw-put-again "${RW[@]}" -X PUT "${J[@]}" -d '{"repositoryUrl":"https://example.com/foo","mainBranch":"main"}' $B/pacticipants/Foo
auth_capture rw-patch "${RW[@]}" -X PATCH "${MP[@]}" -d '{"displayName":"Foo Service"}' $B/pacticipants/Foo
auth_capture bearer -H 'Authorization: Bearer sometoken' $B/
auth_capture options-index -X OPTIONS $B/
"$PB" describe-pacticipant --name Foo --broker-base-url $B --broker-username rw --broker-password rwpass > "$C/describe-pacticipant-Foo.txt"
auth_capture rw-patch-again "${RW[@]}" -X PATCH "${MP[@]}" -d '{"repositoryUrl":"https://example.com/foo","mainBranch":"main"}' $B/pacticipants/Foo
auth_capture rw-put-existing "${RW[@]}" -X PUT "${J[@]}" -d '{"displayName":"Foo Service","repositoryUrl":"https://example.com/foo","mainBranch":"main"}' $B/pacticipants/Foo
auth_capture rw-patch-json-ct "${RW[@]}" -X PATCH "${J[@]}" -d '{"displayName":"Foo Service"}' $B/pacticipants/Foo
auth_capture rw-patch-bad-json "${RW[@]}" -X PATCH "${MP[@]}" -d '{not json' $B/pacticipants/Foo
auth_capture rw-patch-unknown-field "${RW[@]}" -X PATCH "${MP[@]}" -d '{"bogus":1}' $B/pacticipants/Foo
auth_capture rw-delete-missing "${RW[@]}" -X DELETE $B/pacticipants/Nope
auth_capture rw-options-pacticipant "${RW[@]}" -X OPTIONS $B/pacticipants/Foo
auth_capture rw-head-pacticipant "${RW[@]}" -I $B/pacticipants/Foo
pretty < "$A/rw-patch.body" > "$F/PATCH_pacticipants_Foo_created.json"
pretty < "$A/rw-patch-again.body" > "$F/PATCH_pacticipants_Foo_updated.json"

echo "pacticipants through the CLI and POST"
"$PB" create-or-update-pacticipant --name Bar --main-branch trunk --repository-url https://example.com/bar --broker-base-url $B --broker-username rw --broker-password rwpass >/dev/null
"$PB" create-or-update-pacticipant --name Baz --main-branch main --repository-url https://example.com/baz --broker-base-url $B --broker-username rw --broker-password rwpass >/dev/null
"$PB" create-or-update-pacticipant --name Bar --main-branch trunk --repository-url https://example.com/bar2 --broker-base-url $B --broker-username rw --broker-password rwpass >/dev/null
auth_capture post-displayname "${RW[@]}" -X POST "${J[@]}" -d '{"name":"my-service_v2"}' $B/pacticipants
auth_capture post-duplicate "${RW[@]}" -X POST "${J[@]}" -d '{"name":"Bar"}' $B/pacticipants
auth_capture post-no-name "${RW[@]}" -X POST "${J[@]}" -d '{"mainBranch":"main"}' $B/pacticipants
auth_capture post-empty-name "${RW[@]}" -X POST "${J[@]}" -d '{"name":""}' $B/pacticipants
auth_capture post-bad-json "${RW[@]}" -X POST "${J[@]}" -d '{nope' $B/pacticipants 2>/dev/null
auth_capture post-full "${RW[@]}" -X POST "${J[@]}" -d '{"name":"Qux","displayName":"The Qux","repositoryUrl":"https://example.com/qux","mainBranch":"develop"}' $B/pacticipants
pretty < "$A/post-full.body" > "$F/POST_pacticipants_Qux.json"
auth_capture delete-foo "${RW[@]}" -X DELETE $B/pacticipants/Foo
auth_capture get-foo-after-delete "${RW[@]}" $B/pacticipants/Foo
auth_capture post-space "${RW[@]}" -X POST "${J[@]}" -d '{"name":"Example App"}' $B/pacticipants

echo "public read on"
export PACT_BROKER_ALLOW_PUBLIC_READ=true
broker_up
auth_capture public-get $B/
auth_capture public-put -X PUT "${J[@]}" -d '{}' $B/pacticipants/Bar
auth_capture public-options -X OPTIONS $B/
for n in Bar Qux my-service_v2 "Example%20App"; do
  curl -s "$B/pacticipants/$n" | pretty > "$F/GET_pacticipants_$(echo "$n" | sed 's/%20/_/g').json"
done
curl -s $B/pacticipants | pretty > "$F/GET_pacticipants_populated.json"
"$PB" describe-pacticipant --name Bar --broker-base-url $B --broker-username rw --broker-password rwpass > "$C/describe-pacticipant-Bar.txt"

# ---------------------------------------------------------------------------------------
# Phase 2: publish and retrieve. Runs on a fresh, open broker so the data set is
# deterministic. Everything lands under $F/phase2 and $C/phase2 so Phase 1 fixtures keep
# their names. Mutations happen in a fixed order; GETs are captured after all publishes.
# ---------------------------------------------------------------------------------------
echo "phase 2: resetting the Ruby broker"
"${COMPOSE[@]}" down -v >/dev/null 2>&1 || true
unset PACT_BROKER_BASIC_AUTH_USERNAME PACT_BROKER_BASIC_AUTH_PASSWORD \
      PACT_BROKER_BASIC_AUTH_READ_ONLY_USERNAME PACT_BROKER_BASIC_AUTH_READ_ONLY_PASSWORD PACT_BROKER_ALLOW_PUBLIC_READ
"${COMPOSE[@]}" up -d >/dev/null 2>&1
for _ in $(seq 1 90); do curl -sf $B/ >/dev/null && break; sleep 1; done
F2="$F/phase2"; C2="$C/phase2"; mkdir -p "$F2" "$C2"
PACT="$ROOT/tests/fixtures/pact/consumer-provider.json"
TMP="$(mktemp -d)"; trap 'rm -rf "$TMP"' EXIT
J=(-H 'Content-Type: application/json' -H 'Accept: application/hal+json')
# cap <name> <curl args...>: saves $F2/<name>.json (pretty) and $F2/<name>.json.headers
cap() { local n=$1; shift; curl -s -D "$F2/$n.json.headers" "$@" | pretty > "$F2/$n.json"; }
get2() { local p=$1 f; f="GET_$(echo "${p:-index}" | tr '/?=&' '____')"; cap "$f" -H 'Accept: application/hal+json' "$B/$p"; }
b64() { base64 < "$1" | tr -d '\n'; }
# publish <name> <pacticipant> <version> <branch-or-empty> <tags-json-array-or-empty> <contracts-json>
publish() {
  local n=$1 p=$2 v=$3 br=$4 tags=$5 contracts=$6 body
  body="{\"pacticipantName\":\"$p\",\"pacticipantVersionNumber\":\"$v\""
  [ -n "$tags" ] && body="$body,\"tags\":$tags"
  [ -n "$br" ] && body="$body,\"branch\":\"$br\""
  body="$body,\"contracts\":$contracts}"
  cap "$n" "${J[@]}" -X POST -d "$body" $B/contracts/publish
}
contract() { echo "{\"consumerName\":\"$1\",\"providerName\":\"$2\",\"specification\":\"pact\",\"contentType\":\"application/json\",\"content\":\"$3\"}"; }
# Variant pacts: changed response body; a different consumer; a different provider.
python3 -I - "$PACT" "$TMP" <<'EOF'
import json, sys
src, out = sys.argv[1], sys.argv[2]
d = json.load(open(src))
c = json.loads(json.dumps(d)); c["interactions"][0]["response"]["body"]["status"] = "closed"
import os; os.makedirs(f"{out}/changed", exist_ok=True)
json.dump(c, open(f"{out}/changed/consumer-provider.json", "w"), indent=2)
w = json.loads(json.dumps(d)); w["consumer"]["name"] = "Web"
json.dump(w, open(f"{out}/web-provider.json", "w"))
o = json.loads(json.dumps(w)); o["provider"]["name"] = "Other"
json.dump(o, open(f"{out}/web-other.json", "w"))
# Three interactions whose creation order, description order and JSON order all differ,
# to pin down how the pact version sha orders interactions.
ints = [{"description": "zeta", "request": {"method": "GET", "path": "/a"}, "response": {"status": 200}},
        {"description": "alpha", "providerState": "zz", "request": {"method": "GET", "path": "/b"}, "response": {"status": 200}},
        {"description": "mid", "request": {"method": "POST", "path": "/c"}, "response": {"status": 201}}]
json.dump({"consumer": {"name": "Consumer"}, "provider": {"name": "Provider"}, "interactions": ints,
           "metadata": {"pactSpecification": {"version": "2.0.0"}}}, open(f"{out}/three.json", "w"))
json.dump({"consumer": {"name": "Consumer"}, "provider": {"name": "Provider"}, "interactions": [], "metadata": {}}, open(f"{out}/minimal.json", "w"))
json.dump({"consumer": {"name": "Other"}, "provider": {"name": "Provider"}, "interactions": []}, open(f"{out}/mismatch.json", "w"))
json.dump({"interactions": []}, open(f"{out}/nonames.json", "w"))
EOF
C1=$(b64 "$PACT"); C2B=$(b64 "$TMP/changed/consumer-provider.json"); WEBP=$(b64 "$TMP/web-provider.json"); WEBO=$(b64 "$TMP/web-other.json")

echo "phase 2: publish through the CLI (output + proxied request log)"
PROXY_PORT=$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1])')
node "$ROOT/e2e/proxy.js" "$PROXY_PORT" $B "$C2/publish-1.0.0.jsonl" & PROXY=$!
trap 'kill $PROXY 2>/dev/null; rm -rf "$TMP"' EXIT
sleep 0.5
VIA="http://127.0.0.1:$PROXY_PORT"
"$PB" publish "$(dirname "$PACT")" --consumer-app-version 1.0.0 --branch main --tag dev --broker-base-url "$VIA" > "$C2/publish-1.0.0.txt" 2>&1 || true
"$PB" publish "$(dirname "$PACT")" --consumer-app-version 1.0.0 --branch main --tag dev --broker-base-url "$VIA" > "$C2/publish-1.0.0-again.txt" 2>&1 || true
set +e; "$PB" publish "$TMP/changed" --consumer-app-version 1.0.0 --branch main --broker-base-url "$VIA" > "$C2/publish-1.0.0-changed.txt" 2>&1; echo "exit=$?" >> "$C2/publish-1.0.0-changed.txt"; set -e
kill $PROXY 2>/dev/null || true; wait $PROXY 2>/dev/null || true
trap 'rm -rf "$TMP"' EXIT

echo "phase 2: publish-contracts variants"
publish POST_contracts_publish_1.0.0_again Consumer 1.0.0 main '["dev"]' "[$(contract Consumer Provider "$C1")]"
publish POST_contracts_publish_1.0.0_changed Consumer 1.0.0 main '[]' "[$(contract Consumer Provider "$C2B")]"
publish POST_contracts_publish_1.0.1_nobranch Consumer 1.0.1 "" '[]' "[$(contract Consumer Provider "$C1")]"
publish POST_contracts_publish_1.0.2_tags_only Consumer 1.0.2 "" '["prod","test"]' "[$(contract Consumer Provider "$C1")]"
publish POST_contracts_publish_2.0.0_main_changed Consumer 2.0.0 main '[]' "[$(contract Consumer Provider "$C2B")]"
publish POST_contracts_publish_2.0.1_main_same Consumer 2.0.1 main '[]' "[$(contract Consumer Provider "$C2B")]"
publish POST_contracts_publish_3.0.0_feat Consumer 3.0.0 feat '[]' "[$(contract Consumer Provider "$C1")]"
cap POST_contracts_publish_3.0.1_buildurl "${J[@]}" -X POST -d "{\"pacticipantName\":\"Consumer\",\"pacticipantVersionNumber\":\"3.0.1\",\"branch\":\"feat\",\"buildUrl\":\"https://ci.example.com/1\",\"contracts\":[$(contract Consumer Provider "$C1")]}" $B/contracts/publish
publish POST_contracts_publish_two_contracts Web 1 main '["dev"]' "[$(contract Web Provider "$WEBP"),$(contract Web Other "$WEBO")]"
cap POST_contracts_publish_err_empty "${J[@]}" -X POST -d '{}' $B/contracts/publish
publish POST_contracts_publish_err_consumer_mismatch Consumer 9 "" "" "[$(contract Web Provider "$C1")]"
cap POST_contracts_publish_err_bad_base64 "${J[@]}" -X POST -d '{"pacticipantName":"Consumer","pacticipantVersionNumber":"9","contracts":[{"consumerName":"Consumer","providerName":"Provider","specification":"pact","contentType":"application/json","content":"!!!notbase64"}]}' $B/contracts/publish
publish POST_contracts_publish_err_bad_json_content Consumer 9 "" "" "[$(contract Consumer Provider "$(printf 'not json' | base64)")]"
publish POST_contracts_publish_err_content_names_differ Consumer 9 "" "" "[$(contract Consumer Provider "$WEBP")]"
cap POST_contracts_publish_err_not_json "${J[@]}" -X POST -d 'nope' $B/contracts/publish
cap POST_contracts_publish_err_whitespace_version "${J[@]}" -X POST -d '{"pacticipantName":"Consumer","pacticipantVersionNumber":" ","contracts":[]}' $B/contracts/publish
cap POST_contracts_publish_err_bad_types "${J[@]}" -X POST -d '{"pacticipantName":1,"pacticipantVersionNumber":"9","tags":"dev","branch":["x"],"contracts":[{"consumerName":"Consumer","providerName":"Provider","specification":"openapi","contentType":"text/plain","content":"aGk="}]}' $B/contracts/publish
cap POST_contracts_publish_err_wrong_content_type "${J[@]}" -X POST -d "{\"pacticipantName\":\"Consumer\",\"pacticipantVersionNumber\":\"9\",\"contracts\":[{\"consumerName\":\"Consumer\",\"providerName\":\"Provider\",\"specification\":\"pact\",\"contentType\":\"application/xml\",\"content\":\"$C1\"}]}" $B/contracts/publish
cap POST_contracts_publish_err_no_contracts "${J[@]}" -X POST -d '{"pacticipantName":"Consumer","pacticipantVersionNumber":"9","contracts":[]}' $B/contracts/publish
for m in GET PUT OPTIONS; do cap "${m}_contracts_publish" -X $m $B/contracts/publish; done

echo "phase 2: legacy PUT publish"
put() { cap "$1" "${J[@]}" -X PUT --data-binary "@$2" "$B/pacts/provider/Provider/consumer/Consumer/version/$3"; }
put PUT_pact_4.0.0_new "$PACT" 4.0.0
put PUT_pact_4.0.0_same "$PACT" 4.0.0
put PUT_pact_4.0.0_changed "$TMP/changed/consumer-provider.json" 4.0.0
printf 'nope' > "$TMP/notjson"; put PUT_pact_not_json "$TMP/notjson" 4.0.1
put PUT_pact_4.0.2_minimal "$TMP/minimal.json" 4.0.2
put PUT_pact_mismatch "$TMP/mismatch.json" 4.0.3
put PUT_pact_4.0.4_nonames "$TMP/nonames.json" 4.0.4
cap PUT_pact_duplicate_name_case "${J[@]}" -X PUT --data-binary "@$TMP/minimal.json" $B/pacts/provider/Provider/consumer/consumer/version/1
put PUT_pact_1.0.0%2Bbuild "$TMP/minimal.json" "1.0.0%2Bbuild"
put PUT_pact_7.0.0_three_interactions "$TMP/three.json" 7.0.0
for m in HEAD OPTIONS PATCH; do cap "${m}_pact_1.0.0" -X $m $B/pacts/provider/Provider/consumer/Consumer/version/1.0.0; done

echo "phase 2: direct version, tag and branch writes"
cap PUT_version_8.0.0_new "${J[@]}" -X PUT -d '{}' $B/pacticipants/Consumer/versions/8.0.0
cap PUT_version_8.0.0_existing "${J[@]}" -X PUT -d '{"buildUrl":"https://ci/8"}' $B/pacticipants/Consumer/versions/8.0.0
cap PUT_version_new_pacticipant "${J[@]}" -X PUT -d '{}' $B/pacticipants/Brand_New/versions/1
cap PUT_tag_9.0.0_t1_new_version "${J[@]}" -X PUT $B/pacticipants/Consumer/versions/9.0.0/tags/t1
cap PUT_tag_9.0.0_t1_same "${J[@]}" -X PUT $B/pacticipants/Consumer/versions/9.0.0/tags/t1
cap PUT_tag_9.0.0_t2 "${J[@]}" -X PUT $B/pacticipants/Consumer/versions/9.0.0/tags/t2
cap PUT_tag_no_content_type -X PUT $B/pacticipants/Consumer/versions/9.0.0/tags/t3
cap PUT_branch_version_release_10.0.0_new "${J[@]}" -X PUT $B/pacticipants/Consumer/branches/release/versions/10.0.0
cap PUT_branch_version_release_10.0.0_same "${J[@]}" -X PUT $B/pacticipants/Consumer/branches/release/versions/10.0.0
cap PUT_branch_version_release_9.0.0_existing_version "${J[@]}" -X PUT $B/pacticipants/Consumer/branches/release/versions/9.0.0

echo "phase 2: retrieval documents"
for p in "" pacticipants "pacticipants/Consumer" "pacticipants/Consumer/versions" "pacticipants/Consumer/versions/1.0.0" \
  "pacticipants/Consumer/versions/1.0.2" "pacticipants/Consumer/versions/3.0.1" "pacticipants/Consumer/versions/9.0.0" "pacticipants/Consumer/versions/1.0.0%2Bbuild" \
  "pacticipants/Consumer/latest-version" "pacticipants/Consumer/latest-version/dev" "pacticipants/Consumer/latest-version/nope" "pacticipants/Consumer/versions/9.9.9" \
  "pacticipants/Consumer/branches" "pacticipants/Consumer/branches/main" "pacticipants/Consumer/branches/release" "pacticipants/Consumer/branches/nope" \
  "pacticipants/Consumer/branches/main/latest-version" "pacticipants/Consumer/branches/nope/latest-version" \
  "pacticipants/Consumer/branches/main/versions/1.0.0" "pacticipants/Consumer/branches/main/versions/9.9.9" \
  "pacticipants/Consumer/versions/1.0.0/tags/dev" "pacticipants/Consumer/versions/1.0.0/tags/nope" "pacticipants/Consumer/versions/1.0.0/tags" \
  "pacticipants/Provider" "pacticipants/Provider/versions" "pacticipants/Web" "pacticipants/Web/versions/1" "pacticipants/Nope/versions" "pacticipants/Nope/branches" \
  "pacts/latest" "pacts/provider/Provider/consumer/Consumer/version/1.0.0" "pacts/provider/Provider/consumer/Consumer/version/1.0.0%2Bbuild" \
  "pacts/provider/Provider/consumer/Consumer/version/9.9.9" "pacts/provider/Provider/consumer/Consumer/latest" "pacts/provider/Provider/consumer/Consumer/latest/dev" \
  "pacts/provider/Provider/consumer/Consumer/latest/nope" "pacts/provider/Provider/consumer/Consumer/latest-untagged" "pacts/provider/Provider/consumer/Consumer/versions" \
  "pacts/provider/Provider/consumer/Consumer/pact-version/0350e2d94a510324ee31cd7375bf1660e5d8c485" "pacts/provider/Provider/consumer/Consumer/pact-version/deadbeef" \
  "pacts/provider/Provider/consumer/Consumer/tag/dev" "pacts/provider/Provider/consumer/Consumer/branch/main" \
  "pacts/provider/Provider/consumer/Consumer/branch/main/latest/versions" "pacts/provider/Provider/consumer/Consumer/branch/latest/versions" "pacts/provider/Provider/consumer/Consumer/branch/versions" \
  "pacts/provider/Provider/latest" "pacts/provider/Provider/latest/dev" "pacts/provider/Provider" "pacts/provider/Provider/tag/dev" \
  "pacts/provider/Provider/branch" "pacts/provider/Provider/branch/main" "pacts/provider/Provider/branch/latest" "pacts/provider/Provider/branch/main/latest" \
  "pacts/provider/Nope/consumer/Consumer/latest" "pacts/provider/Provider/consumer/Nope/latest" "pacts/provider/Nope/latest" "integrations"; do get2 "$p"; done
cap GET_pact_1.0.0_accept_json -H 'Accept: application/json' $B/pacts/provider/Provider/consumer/Consumer/version/1.0.0
cap GET_pact_1.0.0_accept_any $B/pacts/provider/Provider/consumer/Consumer/version/1.0.0
for p in pacticipants/Consumer/versions/1.0.0 pacticipants/Consumer/versions/1.0.0/tags/dev pacticipants/Consumer/branches/main \
  pacticipants/Consumer/branches/main/versions/1.0.0 pacticipants/Consumer/versions pacts/latest pacts/provider/Provider/consumer/Consumer/latest \
  pacts/provider/Provider/consumer/Consumer/versions pacticipants/Consumer/latest-version pacticipants/Consumer/branches pacts/provider/Provider/latest \
  pacts/provider/Provider/consumer/Consumer/pact-version/0350e2d94a510324ee31cd7375bf1660e5d8c485 pacticipants/Consumer/branches/main/latest-version; do
  cap "OPTIONS_$(echo "$p" | tr '/' '_')" -X OPTIONS "$B/$p"
done

echo "phase 2: CLI reads (1.0.1 is older than the 10 s first-tag-as-branch window by now)"
sleep 11
cli2() { local n=$1; shift; set +e; "$PB" "$@" --broker-base-url $B > "$C2/$n.txt" 2>&1; echo "exit=$?" >> "$C2/$n.txt"; set -e; }
cli2 list-latest-pact-versions list-latest-pact-versions
cli2 list-latest-pact-versions-json list-latest-pact-versions --output json
cli2 describe-version-1.0.0 describe-version --pacticipant Consumer --version 1.0.0
cli2 describe-version-latest describe-version --pacticipant Consumer
cli2 describe-version-latest-dev describe-version --pacticipant Consumer --latest dev
cli2 describe-version-1.0.0-json describe-version --pacticipant Consumer --version 1.0.0 --output json
cli2 describe-pacticipant-Consumer describe-pacticipant --name Consumer
cli2 create-version-tag-1.0.1 create-version-tag --pacticipant Consumer --version 1.0.1 --tag qa --tag uat
cli2 create-version-tag-1.0.1-again create-version-tag --pacticipant Consumer --version 1.0.1 --tag qa
cli2 create-version-tag-auto-5.0.0 create-version-tag --pacticipant Consumer --version 5.0.0 --tag qa --auto-create-version
cli2 create-version-tag-missing-6.0.0 create-version-tag --pacticipant Consumer --version 6.0.0 --tag qa
cli2 create-or-update-version-11.0.0 create-or-update-version --pacticipant Consumer --version 11.0.0 --branch main --tag t3
cli2 describe-version-11.0.0-json describe-version --pacticipant Consumer --version 11.0.0 --output json
cli2 delete-branch-feat delete-branch --pacticipant Consumer --branch feat
cli2 delete-branch-feat-again delete-branch --pacticipant Consumer --branch feat
get2 "pacticipants/Consumer/versions/1.0.1"
get2 "pacticipants/Consumer/versions/5.0.0"

echo "phase 2: deletes"
cap DELETE_tag_9.0.0_t2 "${J[@]}" -X DELETE $B/pacticipants/Consumer/versions/9.0.0/tags/t2
cap DELETE_tag_9.0.0_t2_again "${J[@]}" -X DELETE $B/pacticipants/Consumer/versions/9.0.0/tags/t2
cap DELETE_pact_1.0.0 "${J[@]}" -X DELETE $B/pacts/provider/Provider/consumer/Consumer/version/1.0.0
cap DELETE_pact_1.0.0_again "${J[@]}" -X DELETE $B/pacts/provider/Provider/consumer/Consumer/version/1.0.0
cap DELETE_version_9.0.0 "${J[@]}" -X DELETE $B/pacticipants/Consumer/versions/9.0.0
cap DELETE_version_9.0.0_again "${J[@]}" -X DELETE $B/pacticipants/Consumer/versions/9.0.0
cap DELETE_branch_version_release_10.0.0 "${J[@]}" -X DELETE $B/pacticipants/Consumer/branches/release/versions/10.0.0
cap DELETE_branch_release "${J[@]}" -X DELETE $B/pacticipants/Consumer/branches/release
cap DELETE_pacticipant_Web "${J[@]}" -X DELETE $B/pacticipants/Web
for p in "pacticipants/Consumer/versions/10.0.0" "pacticipants/Consumer/versions/1.0.0" "pacts/latest" "pacticipants" "integrations" "pacticipants/Consumer/versions" "pacticipants/Consumer/branches"; do
  cap "AFTER_DELETES_GET_$(echo "$p" | tr '/' '_')" -H 'Accept: application/hal+json' "$B/$p"
done
echo "done: $(find "$F" "$C" -type f | wc -l | tr -d ' ') files"
