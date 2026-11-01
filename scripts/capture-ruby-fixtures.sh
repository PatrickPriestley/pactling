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
# bulk deletes (added after review): branches with exclusions, an integration's pacts
for v in 1 2 3; do cap "PUT_bulk_branch_$v" "${J[@]}" -X PUT "$B/pacticipants/BulkC/branches/b$v/versions/$v"; done
cap DELETE_branches_exclude -X DELETE "$B/pacticipants/BulkC/branches?exclude%5B%5D=b1&exclude%5B%5D=b2"
cap DELETE_branches_all -X DELETE "$B/pacticipants/BulkC/branches"
cap DELETE_branches_unknown_pacticipant -X DELETE "$B/pacticipants/Nope/branches"
put PUT_pact_bulk_b1 "$TMP/minimal.json" b1
put PUT_pact_bulk_b2 "$TMP/minimal.json" b2
cap DELETE_integration_versions -X DELETE "$B/pacts/provider/Provider/consumer/Consumer/versions"
cap DELETE_integration_versions_again -X DELETE "$B/pacts/provider/Provider/consumer/Consumer/versions"
cap DELETE_integration_versions_unknown_provider -X DELETE "$B/pacts/provider/Nope/consumer/Consumer/versions"
cap DELETE_integration_versions_unknown_consumer -X DELETE "$B/pacts/provider/Provider/consumer/Nope/versions"
# two contracts for one provider in one request: the last one wins (added after review)
cap POST_contracts_publish_duplicate_provider "${J[@]}" -X POST -d "{\"pacticipantName\":\"Consumer\",\"pacticipantVersionNumber\":\"dup1\",\"contracts\":[$(contract Consumer Provider "$C1"),$(contract Consumer Provider "$C2B")]}" $B/contracts/publish
get2 "pacts/provider/Provider/consumer/Consumer/version/dup1"
for p in "pacticipants/Consumer/versions/10.0.0" "pacticipants/Consumer/versions/1.0.0" "pacts/latest" "pacticipants" "integrations" "pacticipants/Consumer/versions" "pacticipants/Consumer/branches"; do
  cap "AFTER_DELETES_GET_$(echo "$p" | tr '/' '_')" -H 'Accept: application/hal+json' "$B/$p"
done

# ---------------------------------------------------------------------------------------
# Phase 3: verification. Fresh broker again; the pact-js 13 provider verifier runs through
# the proxy (e2e/provider.test.js) so its exact requests are kept, then curl probes every
# selector kind, the pending rule and the verification-result documents.
# ---------------------------------------------------------------------------------------
echo "phase 3: resetting the Ruby broker"
"${COMPOSE[@]}" down -v >/dev/null 2>&1 || true
"${COMPOSE[@]}" up -d >/dev/null 2>&1
for _ in $(seq 1 90); do curl -sf $B/ >/dev/null && break; sleep 1; done
F3="$F/phase3"; C3="$C/phase3"; mkdir -p "$F3" "$C3"
cap3() { local n=$1; shift; curl -s -D "$F3/$n.json.headers" "$@" | pretty > "$F3/$n.json"; }
mkdir -p "$TMP/p1" "$TMP/p2" "$TMP/p4" "$TMP/web"
cp "$PACT" "$TMP/p1/"; cp "$TMP/changed/consumer-provider.json" "$TMP/p2/"; cp "$TMP/web-provider.json" "$TMP/web/"
python3 -I - "$PACT" "$TMP/p4/consumer-provider.json" <<'EOF'
import json, sys
d = json.load(open(sys.argv[1])); d["interactions"][0]["response"]["body"]["id"] = 4
json.dump(d, open(sys.argv[2], "w"))
EOF
"$PB" publish "$TMP/p1" --consumer-app-version 1.0.0 --branch main --tag dev --broker-base-url $B >/dev/null
"$PB" publish "$TMP/p2" --consumer-app-version 2.0.0 --branch feat --broker-base-url $B >/dev/null
"$PB" publish "$TMP/web" --consumer-app-version 1 --branch main --broker-base-url $B >/dev/null
"$PB" publish "$TMP/p4" --consumer-app-version 4.0.0 --branch zeta --broker-base-url $B >/dev/null

echo "phase 3: pact-js provider verification through the proxy"
verify_via_proxy() { # <log name> <env assignments...>
  local n=$1; shift
  local port; port=$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1])')
  node "$ROOT/e2e/proxy.js" "$port" $B "$C3/$n.jsonl" & local pid=$!
  trap 'kill '"$pid"' 2>/dev/null; rm -rf "$TMP"' EXIT
  sleep 0.5
  set +e; env BROKER="http://127.0.0.1:$port" "$@" node --test "$ROOT/e2e/provider.test.js" > "$C3/$n.txt" 2>&1; echo "exit=$?" >> "$C3/$n.txt"; set -e
  kill $pid 2>/dev/null || true; wait $pid 2>/dev/null || true
  trap 'rm -rf "$TMP"' EXIT
}
verify_via_proxy verify-main-v1 PROVIDER_VERSION=1 PROVIDER_BRANCH=main
verify_via_proxy verify-main-v2-fail PROVIDER_VERSION=2 PROVIDER_BRANCH=main FAIL=true
verify_via_proxy verify-feat-v3 PROVIDER_VERSION=3 PROVIDER_BRANCH=feat
verify_via_proxy verify-feat-v6-zeta PROVIDER_VERSION=6 PROVIDER_BRANCH=feat ORDER_ID=4 'SELECTORS=[{"branch":"zeta"}]'

echo "phase 3: selector and pending probes"
FV="$B/pacts/provider/Provider/for-verification"
cap3 GET_for-verification -X GET "${J[@]}" "$FV"
sel() { local n=$1; shift; cap3 "POST_for-verification_$n" "${J[@]}" -X POST -d "$1" "$FV"; }
sel empty '{}'
sel empty_array '{"consumerVersionSelectors":[]}'
sel latest '{"consumerVersionSelectors":[{"latest":true}]}'
sel tag '{"consumerVersionSelectors":[{"tag":"dev"}]}'
sel tag_latest '{"consumerVersionSelectors":[{"tag":"dev","latest":true}]}'
sel tag_nope '{"consumerVersionSelectors":[{"tag":"nope"}]}'
sel branch '{"consumerVersionSelectors":[{"branch":"feat"}]}'
sel branch_consumer '{"consumerVersionSelectors":[{"branch":"main","consumer":"Web"}]}'
sel mainBranch '{"consumerVersionSelectors":[{"mainBranch":true}]}'
sel matchingBranch_no_provider_branch '{"consumerVersionSelectors":[{"matchingBranch":true}]}'
sel matchingBranch_feat '{"consumerVersionSelectors":[{"matchingBranch":true}],"providerVersionBranch":"feat"}'
sel deployed '{"consumerVersionSelectors":[{"deployed":true}]}'
sel released '{"consumerVersionSelectors":[{"released":true}]}'
sel deployedOrReleased '{"consumerVersionSelectors":[{"deployedOrReleased":true}]}'
sel two_selectors '{"includePendingStatus":true,"consumerVersionSelectors":[{"mainBranch":true},{"deployedOrReleased":true},{"matchingBranch":true}],"providerVersionBranch":"main"}'
sel pending_from_feat '{"includePendingStatus":true,"consumerVersionSelectors":[{"branch":"zeta"}],"providerVersionBranch":"feat"}'
sel pending_from_unknown_branch '{"includePendingStatus":true,"consumerVersionSelectors":[{"branch":"zeta"}],"providerVersionBranch":"x"}'
sel pending_from_main '{"includePendingStatus":true,"consumerVersionSelectors":[{"branch":"zeta"}],"providerVersionBranch":"main"}'
sel pending_no_branch '{"includePendingStatus":true,"consumerVersionSelectors":[{"branch":"zeta"}]}'
sel pending_provider_tags '{"includePendingStatus":true,"consumerVersionSelectors":[{"mainBranch":true}],"providerVersionTags":["prod"]}'
sel pending_off '{"consumerVersionSelectors":[{"branch":"zeta"}],"providerVersionBranch":"main"}'
sel err_bogus_selector '{"consumerVersionSelectors":[{"bogus":true}]}'
sel err_types '{"consumerVersionSelectors":"x","includePendingStatus":"yes"}'
sel err_not_json 'nope'
cap3 POST_for-verification_unknown_provider -X POST "${J[@]}" -d '{}' "$B/pacts/provider/Nope/for-verification"
cap3 OPTIONS_for-verification -X OPTIONS "$FV"

echo "phase 3: verification result documents"
PV="$B/pacts/provider/Provider/consumer/Consumer/pact-version/0350e2d94a510324ee31cd7375bf1660e5d8c485"
cap3 GET_verification-results_latest "${J[@]}" "$PV/verification-results/latest"
cap3 GET_verification-results_1 "${J[@]}" "$PV/verification-results/1"
cap3 GET_verification-results_999 "${J[@]}" "$PV/verification-results/999"
cap3 GET_verification-results_unknown_pact "${J[@]}" "$B/pacts/provider/Provider/consumer/Consumer/pact-version/deadbeef/verification-results/latest"
cap3 GET_pact_by_version_verification-results_latest "${J[@]}" "$B/pacts/provider/Provider/consumer/Consumer/version/1.0.0/verification-results/latest"
cap3 GET_consumer_version_verification-results_latest "${J[@]}" "$B/verification-results/consumer/Consumer/version/1.0.0/latest"
cap3 GET_consumer_version_verification-results_latest_unknown "${J[@]}" "$B/verification-results/consumer/Consumer/version/9.9.9/latest"
cap3 POST_verification-results_minimal "${J[@]}" -X POST -d '{"providerApplicationVersion":"7","success":true}' "$PV/verification-results"
# the successful numeric read, through the Location the POST returned (added after review)
LOC=$(grep -i '^location:' "$F3/POST_verification-results_minimal.json.headers" | tr -d '\r' | cut -d' ' -f2)
cap3 GET_verification-results_by_number "${J[@]}" "$LOC"
cap3 POST_verification-results_err_empty "${J[@]}" -X POST -d '{}' "$PV/verification-results"
cap3 POST_verification-results_err_types "${J[@]}" -X POST -d '{"providerApplicationVersion":"","success":"yes"}' "$PV/verification-results"
cap3 POST_verification-results_err_not_json "${J[@]}" -X POST -d 'nope' "$PV/verification-results"
cap3 POST_verification-results_unknown_pact "${J[@]}" -X POST -d '{"providerApplicationVersion":"7","success":true}' "$B/pacts/provider/Provider/consumer/Consumer/pact-version/deadbeef/verification-results"
cap3 GET_pacticipants_Provider_versions_7 "${J[@]}" "$B/pacticipants/Provider/versions/7"
cap3 GET_pacticipants_Provider_versions "${J[@]}" "$B/pacticipants/Provider/versions"
cap3 GET_pact_1.0.0_after_verification "${J[@]}" "$B/pacts/provider/Provider/consumer/Consumer/version/1.0.0"
for p in "verification-results" "verification-results/1" "verification-results/latest"; do cap3 "OPTIONS_$(echo "$p" | tr '/' '_')" -X OPTIONS "$PV/$p"; done
"$PB" describe-version --pacticipant Provider --version 1 --broker-base-url $B > "$C3/describe-version-Provider-1.txt" 2>&1 || true

# ---------------------------------------------------------------------------------------
# Phase 4: environments, deployments, releases, matrix and can-i-deploy. Fresh broker;
# CLI commands run through the proxy; matrix documents are captured with curl using the
# exact query strings the CLI sends.
# ---------------------------------------------------------------------------------------
echo "phase 4: resetting the Ruby broker"
"${COMPOSE[@]}" down -v >/dev/null 2>&1 || true
"${COMPOSE[@]}" up -d >/dev/null 2>&1
for _ in $(seq 1 90); do curl -sf $B/ >/dev/null && break; sleep 1; done
F4="$F/phase4"; C4="$C/phase4"; mkdir -p "$F4" "$C4"
cap4() { local n=$1; shift; curl -s -D "$F4/$n.json.headers" "$@" | pretty > "$F4/$n.json"; }
cli4() { # <name> <cli args...>: runs the CLI through the proxy, keeping stdout and the traffic
  local n=$1; shift
  local port; port=$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1])')
  node "$ROOT/e2e/proxy.js" "$port" $B "$C4/$n.jsonl" & local pid=$!
  trap 'kill '"$pid"' 2>/dev/null; rm -rf "$TMP"' EXIT
  sleep 0.4
  set +e; "$PB" "$@" --broker-base-url "http://127.0.0.1:$port" > "$C4/$n.txt" 2>&1; echo "exit=$?" >> "$C4/$n.txt"; set -e
  kill $pid 2>/dev/null || true; wait $pid 2>/dev/null || true
  trap 'rm -rf "$TMP"' EXIT
}
matrix4() { cap4 "GET_matrix_$1" "${J[@]}" "$B/matrix?$2"; }
mkdir -p "$TMP/p1" "$TMP/p2" "$TMP/web"; cp "$PACT" "$TMP/p1/"; cp "$TMP/changed/consumer-provider.json" "$TMP/p2/"; cp "$TMP/web-provider.json" "$TMP/web/"
"$PB" publish "$TMP/p1" --consumer-app-version 1.0.0 --branch main --tag dev --broker-base-url $B >/dev/null
"$PB" publish "$TMP/web" --consumer-app-version 1 --branch main --broker-base-url $B >/dev/null
BROKER=$B PROVIDER_VERSION=1 PROVIDER_BRANCH=main node --test "$ROOT/e2e/provider.test.js" >/dev/null 2>&1 || true

echo "phase 4: environments"
cli4 create-environment-test create-environment --name test --display-name "Test" --no-production
cli4 create-environment-production create-environment --name production --display-name "Production" --production --contact-name "Ops" --contact-email-address ops@example.com
cli4 create-environment-duplicate create-environment --name test
cli4 list-environments list-environments
cli4 list-environments-json list-environments --output json
TEST_UUID=$(sed '/^exit=/d' "$C4/list-environments-json.txt" | python3 -c 'import json,sys; d=json.load(sys.stdin); print([e["uuid"] for e in d["_embedded"]["environments"] if e["name"]=="test"][0])')
PROD_UUID=$(sed '/^exit=/d' "$C4/list-environments-json.txt" | python3 -c 'import json,sys; d=json.load(sys.stdin); print([e["uuid"] for e in d["_embedded"]["environments"] if e["name"]=="production"][0])')
cli4 describe-environment describe-environment --uuid "$TEST_UUID"
cli4 update-environment update-environment --uuid "$TEST_UUID" --display-name "Test env" --contact-name "QA"
cap4 GET_environments "${J[@]}" "$B/environments"
cap4 GET_environment_test "${J[@]}" "$B/environments/$TEST_UUID"
cap4 GET_environment_unknown "${J[@]}" "$B/environments/00000000-0000-0000-0000-000000000000"
cap4 POST_environments_err_empty "${J[@]}" -X POST -d '{}' "$B/environments"
cap4 POST_environments_err_types "${J[@]}" -X POST -d '{"name":"bad name!","production":"yes"}' "$B/environments"
cap4 POST_environments_err_no_production "${J[@]}" -X POST -d '{"name":"ok","displayName":""}' "$B/environments"
cap4 PUT_environment_unknown "${J[@]}" -X PUT -d '{"name":"x","production":false}' "$B/environments/00000000-0000-0000-0000-000000000000"
for p in "environments" "environments/$TEST_UUID"; do cap4 "OPTIONS_$(echo "$p" | tr '/' '_' | sed "s/$TEST_UUID/uuid/")" -X OPTIONS "$B/$p"; done

echo "phase 4: deployments and releases"
cli4 record-deployment-provider record-deployment --pacticipant Provider --version 1 --environment test
cli4 record-deployment-consumer record-deployment --pacticipant Consumer --version 1.0.0 --environment test
cli4 record-deployment-again record-deployment --pacticipant Consumer --version 1.0.0 --environment test
cli4 record-deployment-unknown-environment record-deployment --pacticipant Consumer --version 1.0.0 --environment nope
cli4 record-deployment-unknown-version record-deployment --pacticipant Consumer --version 9.9.9 --environment test
cli4 record-deployment-instance record-deployment --pacticipant Web --version 1 --environment test --application-instance blue
cap4 GET_pacticipants_Consumer_versions_1.0.0 "${J[@]}" "$B/pacticipants/Consumer/versions/1.0.0"
cap4 GET_currently-deployed "${J[@]}" "$B/environments/$TEST_UUID/deployed-versions/currently-deployed"
cap4 GET_currently-deployed_consumer "${J[@]}" "$B/environments/$TEST_UUID/deployed-versions/currently-deployed?pacticipant=Consumer"
cap4 GET_currently-supported_empty "${J[@]}" "$B/environments/$TEST_UUID/released-versions/currently-supported"
DV=$(python3 -c 'import json; d=json.load(open("'"$F4"'/GET_currently-deployed_consumer.json")); print(d["_embedded"]["deployedVersions"][0]["uuid"])')
cap4 GET_deployed-version "${J[@]}" "$B/deployed-versions/$DV"
cap4 GET_deployed-version_unknown "${J[@]}" "$B/deployed-versions/00000000-0000-0000-0000-000000000000"
cap4 POST_deployed-version_unknown_version "${J[@]}" -X POST -d '{}' "$B/pacticipants/Consumer/versions/9.9.9/deployed-versions/environment/$TEST_UUID"
cap4 POST_deployed-version_unknown_environment "${J[@]}" -X POST -d '{}' "$B/pacticipants/Consumer/versions/1.0.0/deployed-versions/environment/00000000-0000-0000-0000-000000000000"
cap4 OPTIONS_deployed-version -X OPTIONS "$B/deployed-versions/$DV"
cap4 OPTIONS_currently-deployed -X OPTIONS "$B/environments/$TEST_UUID/deployed-versions/currently-deployed"
cap4 OPTIONS_record-deployment -X OPTIONS "$B/pacticipants/Consumer/versions/1.0.0/deployed-versions/environment/$TEST_UUID"

echo "phase 4: can-i-deploy"
cli4 can-i-deploy-yes can-i-deploy --pacticipant Consumer --version 1.0.0 --to-environment test
cli4 can-i-deploy-yes-json can-i-deploy --pacticipant Consumer --version 1.0.0 --to-environment test --output json
matrix4 consumer_1.0.0_test 'q%5B%5D%5Bpacticipant%5D=Consumer&q%5B%5D%5Bversion%5D=1.0.0&latestby=cvp&environment=test'
"$PB" publish "$TMP/p2" --consumer-app-version 2.0.0 --branch main --broker-base-url $B >/dev/null
cli4 can-i-deploy-unverified can-i-deploy --pacticipant Consumer --version 2.0.0 --to-environment test
matrix4 consumer_2.0.0_test_unverified 'q%5B%5D%5Bpacticipant%5D=Consumer&q%5B%5D%5Bversion%5D=2.0.0&latestby=cvp&environment=test'
# provider v2 on main fails everything it verifies; then provider v1 verifies 2.0.0 and fails (its answers fit 1.0.0 only)
BROKER=$B PROVIDER_VERSION=2 PROVIDER_BRANCH=main FAIL=true node --test "$ROOT/e2e/provider.test.js" >/dev/null 2>&1 || true
BROKER=$B PROVIDER_VERSION=1 PROVIDER_BRANCH=main SELECTORS='[{"branch":"main"}]' node --test "$ROOT/e2e/provider.test.js" >/dev/null 2>&1 || true
cli4 can-i-deploy-failed can-i-deploy --pacticipant Consumer --version 2.0.0 --to-environment test
matrix4 consumer_2.0.0_test_failed 'q%5B%5D%5Bpacticipant%5D=Consumer&q%5B%5D%5Bversion%5D=2.0.0&latestby=cvp&environment=test'
cli4 can-i-deploy-provider-yes can-i-deploy --pacticipant Provider --version 1 --to-environment test
matrix4 provider_1_test 'q%5B%5D%5Bpacticipant%5D=Provider&q%5B%5D%5Bversion%5D=1&latestby=cvp&environment=test'
cli4 can-i-deploy-provider-failed can-i-deploy --pacticipant Provider --version 2 --to-environment test
matrix4 provider_2_test 'q%5B%5D%5Bpacticipant%5D=Provider&q%5B%5D%5Bversion%5D=2&latestby=cvp&environment=test'
cli4 can-i-deploy-unknown-version can-i-deploy --pacticipant Consumer --version 9.9.9 --to-environment test
matrix4 consumer_9.9.9_test 'q%5B%5D%5Bpacticipant%5D=Consumer&q%5B%5D%5Bversion%5D=9.9.9&latestby=cvp&environment=test'
matrix4 unknown_environment 'q%5B%5D%5Bpacticipant%5D=Consumer&q%5B%5D%5Bversion%5D=1.0.0&latestby=cvp&environment=nope'
matrix4 unknown_pacticipant 'q%5B%5D%5Bpacticipant%5D=Nope&q%5B%5D%5Bversion%5D=1&latestby=cvp&environment=test'
cli4 can-i-deploy-no-to can-i-deploy --pacticipant Consumer --version 1.0.0
matrix4 consumer_1.0.0_latest 'q%5B%5D%5Bpacticipant%5D=Consumer&q%5B%5D%5Bversion%5D=1.0.0&latestby=cvp&latest=true'
cli4 can-i-deploy-main-branch can-i-deploy --pacticipant Consumer --main-branch --to-environment test
matrix4 consumer_main_branch_test 'q%5B%5D%5Bpacticipant%5D=Consumer&q%5B%5D%5Blatest%5D=true&q%5B%5D%5BmainBranch%5D=true&latestby=cvp&environment=test'
cli4 can-i-deploy-latest-tag can-i-deploy --pacticipant Consumer --latest dev --to-environment test
matrix4 consumer_latest_dev_test 'q%5B%5D%5Bpacticipant%5D=Consumer&q%5B%5D%5Blatest%5D=true&q%5B%5D%5Btag%5D=dev&latestby=cvp&environment=test'
cli4 can-i-deploy-branch can-i-deploy --pacticipant Consumer --branch main --to-environment test
matrix4 consumer_branch_main_test 'q%5B%5D%5Bpacticipant%5D=Consumer&q%5B%5D%5Blatest%5D=true&q%5B%5D%5Bbranch%5D=main&latestby=cvp&environment=test'
cli4 can-i-deploy-to-tag-missing can-i-deploy --pacticipant Consumer --version 1.0.0 --to prod
matrix4 consumer_1.0.0_to_prod_missing 'q%5B%5D%5Bpacticipant%5D=Consumer&q%5B%5D%5Bversion%5D=1.0.0&latestby=cvp&latest=true&tag=prod'
"$PB" create-version-tag --pacticipant Provider --version 1 --tag prod --broker-base-url $B >/dev/null
cli4 can-i-deploy-to-tag can-i-deploy --pacticipant Consumer --version 1.0.0 --to prod
matrix4 consumer_1.0.0_to_prod 'q%5B%5D%5Bpacticipant%5D=Consumer&q%5B%5D%5Bversion%5D=1.0.0&latestby=cvp&latest=true&tag=prod'
cli4 can-i-deploy-two can-i-deploy --pacticipant Consumer --version 1.0.0 --pacticipant Provider --version 1
matrix4 consumer_1.0.0_provider_1 'q%5B%5D%5Bpacticipant%5D=Consumer&q%5B%5D%5Bversion%5D=1.0.0&q%5B%5D%5Bpacticipant%5D=Provider&q%5B%5D%5Bversion%5D=1&latestby=cvpv'
matrix4 consumer_1.0.0_provider_2 'q%5B%5D%5Bpacticipant%5D=Consumer&q%5B%5D%5Bversion%5D=1.0.0&q%5B%5D%5Bpacticipant%5D=Provider&q%5B%5D%5Bversion%5D=2&latestby=cvpv'
cli4 can-i-deploy-ignore can-i-deploy --pacticipant Consumer --version 2.0.0 --to-environment test --ignore Provider
matrix4 consumer_2.0.0_test_ignore_provider 'q%5B%5D%5Bpacticipant%5D=Consumer&q%5B%5D%5Bversion%5D=2.0.0&latestby=cvp&environment=test&ignore%5B%5D%5Bpacticipant%5D=Provider'
cli4 can-i-deploy-dry-run can-i-deploy --pacticipant Consumer --version 9.9.9 --to-environment test --dry-run
cli4 can-i-deploy-empty-environment can-i-deploy --pacticipant Consumer --version 1.0.0 --to-environment production
matrix4 consumer_1.0.0_production_empty 'q%5B%5D%5Bpacticipant%5D=Consumer&q%5B%5D%5Bversion%5D=1.0.0&latestby=cvp&environment=production'
matrix4 consumer_1.0.0_no_latestby 'q%5B%5D%5Bpacticipant%5D=Consumer&q%5B%5D%5Bversion%5D=1.0.0'
matrix4 consumer_only 'q%5B%5D%5Bpacticipant%5D=Consumer'
cap4 GET_matrix_integration "${J[@]}" "$B/matrix/provider/Provider/consumer/Consumer"
cap4 GET_can-i-deploy_environment "${J[@]}" "$B/can-i-deploy?pacticipant=Consumer&version=1.0.0&environment=test"
cap4 GET_can-i-deploy_to_tag "${J[@]}" "$B/can-i-deploy?pacticipant=Consumer&version=1.0.0&to=prod"
cap4 OPTIONS_matrix -X OPTIONS "$B/matrix"
cap4 POST_for-verification_deployed "${J[@]}" -X POST -d '{"consumerVersionSelectors":[{"deployed":true}]}' "$B/pacts/provider/Provider/for-verification"
cap4 POST_for-verification_deployedOrReleased "${J[@]}" -X POST -d '{"consumerVersionSelectors":[{"deployedOrReleased":true}]}' "$B/pacts/provider/Provider/for-verification"
cap4 POST_for-verification_environment "${J[@]}" -X POST -d '{"consumerVersionSelectors":[{"environment":"test"}]}' "$B/pacts/provider/Provider/for-verification"
cap4 GET_for-verification_with_deployments "${J[@]}" "$B/pacts/provider/Provider/for-verification"

echo "phase 4: releases, support ended, undeployment"
cli4 record-release record-release --pacticipant Consumer --version 1.0.0 --environment production
cli4 record-release-again record-release --pacticipant Consumer --version 1.0.0 --environment production
cap4 GET_currently-supported "${J[@]}" "$B/environments/$PROD_UUID/released-versions/currently-supported"
cap4 GET_currently-supported_filtered "${J[@]}" "$B/environments/$PROD_UUID/released-versions/currently-supported?pacticipant=Consumer&version=1.0.0"
RV=$(python3 -c 'import json; d=json.load(open("'"$F4"'/GET_currently-supported.json")); print(d["_embedded"]["releasedVersions"][0]["uuid"])')
cap4 GET_released-version "${J[@]}" "$B/released-versions/$RV"
cap4 POST_for-verification_released "${J[@]}" -X POST -d '{"consumerVersionSelectors":[{"released":true}]}' "$B/pacts/provider/Provider/for-verification"
cap4 GET_pacticipants_Consumer_versions_1.0.0_released "${J[@]}" "$B/pacticipants/Consumer/versions/1.0.0"
cli4 record-support-ended record-support-ended --pacticipant Consumer --version 1.0.0 --environment production
cli4 record-support-ended-again record-support-ended --pacticipant Consumer --version 1.0.0 --environment production
cli4 record-undeployment record-undeployment --pacticipant Consumer --environment test
cli4 record-undeployment-again record-undeployment --pacticipant Consumer --environment test
cli4 record-undeployment-instance record-undeployment --pacticipant Web --environment test --application-instance blue
cli4 record-deployment-newer record-deployment --pacticipant Consumer --version 2.0.0 --environment test
cap4 GET_pacticipants_Consumer_versions_1.0.0_after "${J[@]}" "$B/pacticipants/Consumer/versions/1.0.0"
cap4 GET_pacticipants_Consumer_versions_2.0.0_after "${J[@]}" "$B/pacticipants/Consumer/versions/2.0.0"
cap4 GET_currently-deployed_after "${J[@]}" "$B/environments/$TEST_UUID/deployed-versions/currently-deployed"
cap4 GET_deployed-version_undeployed "${J[@]}" "$B/deployed-versions/$DV"
cap4 GET_released-version_support_ended "${J[@]}" "$B/released-versions/$RV"
cli4 delete-environment delete-environment --uuid "$PROD_UUID"
cap4 GET_environments_after_delete "${J[@]}" "$B/environments"
python3 "$ROOT/scripts/capture-matrix-regressions.py" "$B" "$F4/matrix-regressions.json"

# ---------------------------------------------------------------------------------------
# Phase 5: webhooks. A fresh broker that may call plain-http receivers on the host; two
# receivers record what the broker sends (e2e/receiver.js), one answering 200 and one 500.
# Linux hosts need host.docker.internal mapped (compose extra_hosts) for the broker to reach them.
# ---------------------------------------------------------------------------------------
echo "phase 5: resetting the Ruby broker with http webhooks allowed"
"${COMPOSE[@]}" down -v >/dev/null 2>&1 || true
export PACT_BROKER_WEBHOOK_SCHEME_WHITELIST=http
"${COMPOSE[@]}" up -d >/dev/null 2>&1
for _ in $(seq 1 90); do curl -sf $B/ >/dev/null && break; sleep 1; done
F5="$F/phase5"; C5="$C/phase5"; mkdir -p "$F5" "$C5"; rm -f "$C5"/receiver*.jsonl
node "$ROOT/e2e/receiver.js" 18100 "$C5/receiver.jsonl" >/dev/null 2>&1 & RECV_OK=$!
RECEIVER_STATUS=500 node "$ROOT/e2e/receiver.js" 18101 "$C5/receiver-failing.jsonl" >/dev/null 2>&1 & RECV_BAD=$!
trap 'kill $RECV_OK $RECV_BAD 2>/dev/null; rm -rf "$TMP"' EXIT
sleep 0.5
HOOK="http://host.docker.internal:18100"; HOOK_BAD="http://host.docker.internal:18101"
cap5() { local n=$1; shift; curl -s -D "$F5/$n.json.headers" "$@" | pretty > "$F5/$n.json"; }
cli5() { # <name> <cli args...>: runs the CLI through the proxy, keeping stdout and the traffic
  local n=$1; shift
  local port; port=$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1])')
  node "$ROOT/e2e/proxy.js" "$port" $B "$C5/$n.jsonl" & local pid=$!
  sleep 0.4
  set +e; "$PB" "$@" --broker-base-url "http://127.0.0.1:$port" > "$C5/$n.txt" 2>&1; echo "exit=$?" >> "$C5/$n.txt"; set -e
  kill $pid 2>/dev/null || true; wait $pid 2>/dev/null || true
}
webhook_uuid() { # <n>: the uuid of the n-th webhook in the list
  curl -s $B/webhooks | python3 -c 'import json,sys; d=json.load(sys.stdin); print(d["_links"]["pb:webhooks"]['"$1"']["href"].rsplit("/",1)[1])'
}
# Every template parameter the Ruby broker documents, so the receiver shows its expansion.
BODY='{"event":"${pactbroker.eventName}","consumer":"${pactbroker.consumerName}","provider":"${pactbroker.providerName}","consumerVersion":"${pactbroker.consumerVersionNumber}","consumerBranch":"${pactbroker.consumerVersionBranch}","consumerTags":"${pactbroker.consumerVersionTags}","providerVersion":"${pactbroker.providerVersionNumber}","providerBranch":"${pactbroker.providerVersionBranch}","providerTags":"${pactbroker.providerVersionTags}","providerVersionDescriptions":"${pactbroker.providerVersionDescriptions}","pactUrl":"${pactbroker.pactUrl}","verificationResultUrl":"${pactbroker.verificationResultUrl}","github":"${pactbroker.githubVerificationStatus}","bitbucket":"${pactbroker.bitbucketVerificationStatus}","azure":"${pactbroker.azureDevOpsVerificationStatus}","gitlab":"${pactbroker.gitlabVerificationStatus}","buildUrl":"${pactbroker.buildUrl}","consumerLabels":"${pactbroker.consumerLabels}","providerLabels":"${pactbroker.providerLabels}","deployedProvider":"${pactbroker.currentlyDeployedProviderVersionNumber}"}'

echo "phase 5: seed (pacts, one verification, a deployment, a second provider version on main)"
mkdir -p "$TMP/p1" "$TMP/p3" "$TMP/web"; cp "$PACT" "$TMP/p1/"; cp "$TMP/web-provider.json" "$TMP/web/"
python3 - "$PACT" "$TMP/p3/consumer-provider.json" <<'PY'
import json, sys
d = json.load(open(sys.argv[1]))
d["interactions"][0]["response"]["body"]["status"] = "shipped"
json.dump(d, open(sys.argv[2], "w"), indent=2)
PY
"$PB" publish "$TMP/p1" --consumer-app-version 1.0.0 --branch main --tag dev --broker-base-url $B >/dev/null
"$PB" publish "$TMP/web" --consumer-app-version 1 --branch main --broker-base-url $B >/dev/null
BROKER=$B PROVIDER_VERSION=1 PROVIDER_BRANCH=main node --test "$ROOT/e2e/provider.test.js" >/dev/null 2>&1 || true
"$PB" create-environment --name test --no-production --broker-base-url $B >/dev/null
"$PB" record-deployment --pacticipant Provider --version 1 --environment test --broker-base-url $B >/dev/null
"$PB" create-or-update-version --pacticipant Provider --version 2 --branch main --broker-base-url $B >/dev/null

echo "phase 5: webhook CRUD"
cli5 create-webhook-consumer-provider create-webhook "$HOOK/hook/\${pactbroker.consumerName}" -X POST -H "Content-Type: application/json" -H "Authorization: Bearer hook-secret" -d "$BODY" --consumer Consumer --provider Provider --description "Trigger Provider build" --contract-content-changed
cli5 create-webhook-provider-all-events create-webhook "$HOOK/hook/all" -X POST -H "Content-Type: application/json" -d "$BODY" --provider Provider --contract-published --contract-content-changed --provider-verification-published --provider-verification-succeeded --provider-verification-failed --contract-requiring-verification-published
cli5 create-webhook-bad-method create-webhook "$HOOK/hook/get" -X GET --contract-content-changed
cli5 create-webhook-https create-webhook "https://example.com/hook" -X POST --contract-content-changed
cli5 create-webhook-unknown-consumer create-webhook "$HOOK/hook/x" -X POST --consumer Nope --contract-content-changed
cli5 create-or-update-webhook-create create-or-update-webhook "$HOOK/hook/upsert" --uuid pactling-capture-webhook-1 -X POST -d '{"event":"${pactbroker.eventName}"}' --consumer Consumer --provider Provider --contract-published
cli5 create-or-update-webhook-update create-or-update-webhook "$HOOK/hook/upsert" --uuid pactling-capture-webhook-1 -X POST -d '{"event":"${pactbroker.eventName}"}' --consumer Consumer --provider Provider --contract-published --description "Updated"
cap5 POST_webhooks_global "${J[@]}" -X POST -d '{"request":{"method":"POST","url":"'"$HOOK"'/hook/global","username":"hookuser","password":"hookpass","headers":{"X-Token":"abc","Accept":"application/json"},"body":{"template":"${pactbroker.consumerVersionNumber}"}}}' $B/webhooks
cap5 POST_webhooks_err_empty "${J[@]}" -X POST -d '{}' $B/webhooks
cap5 POST_webhooks_err_request_empty "${J[@]}" -X POST -d '{"request":{}}' $B/webhooks
cap5 POST_webhooks_err_method "${J[@]}" -X POST -d '{"request":{"method":"GET","url":"'"$HOOK"'/hook"}}' $B/webhooks
cap5 POST_webhooks_err_scheme "${J[@]}" -X POST -d '{"request":{"method":"POST","url":"https://example.com/hook"}}' $B/webhooks
cap5 POST_webhooks_err_url "${J[@]}" -X POST -d '{"request":{"method":"POST","url":"nope"}}' $B/webhooks
cap5 POST_webhooks_err_templated_host "${J[@]}" -X POST -d '{"request":{"method":"POST","url":"http://${pactbroker.consumerName}.example.com/hook"}}' $B/webhooks
cap5 POST_webhooks_err_event "${J[@]}" -X POST -d '{"request":{"method":"POST","url":"'"$HOOK"'/hook"},"events":[{"name":"bogus"}]}' $B/webhooks
cap5 POST_webhooks_err_events_empty "${J[@]}" -X POST -d '{"request":{"method":"POST","url":"'"$HOOK"'/hook"},"events":[]}' $B/webhooks
cap5 POST_webhooks_err_unknown_consumer "${J[@]}" -X POST -d '{"consumer":{"name":"Nope"},"request":{"method":"POST","url":"'"$HOOK"'/hook"}}' $B/webhooks
cap5 POST_webhooks_err_name_and_label "${J[@]}" -X POST -d '{"consumer":{"name":"Consumer","label":"x"},"request":{"method":"POST","url":"'"$HOOK"'/hook"}}' $B/webhooks
cap5 POST_webhooks_err_bad_json "${J[@]}" -X POST -d '{nope' $B/webhooks
cap5 PUT_webhooks_err_uuid "${J[@]}" -X PUT -d '{"request":{"method":"POST","url":"'"$HOOK"'/hook"}}' $B/webhooks/short
W_CP=$(webhook_uuid 0); W_ALL=$(webhook_uuid 1); W_UP=pactling-capture-webhook-1; W_GLOBAL=$(python3 -c 'import json; print(json.load(open("'"$F5"'/POST_webhooks_global.json"))["uuid"])')
cap5 GET_webhooks "${J[@]}" $B/webhooks
cap5 GET_webhook_consumer_provider "${J[@]}" "$B/webhooks/$W_CP"
cap5 GET_webhook_provider_all_events "${J[@]}" "$B/webhooks/$W_ALL"
cap5 GET_webhook_global "${J[@]}" "$B/webhooks/$W_GLOBAL"
cap5 GET_webhook_upsert "${J[@]}" "$B/webhooks/$W_UP"
cap5 GET_webhooks_provider_Provider_consumer_Consumer "${J[@]}" $B/webhooks/provider/Provider/consumer/Consumer
cap5 GET_webhooks_provider_Provider "${J[@]}" $B/webhooks/provider/Provider
cap5 GET_webhooks_consumer_Consumer "${J[@]}" $B/webhooks/consumer/Consumer
cap5 GET_webhooks_consumer_Nope "${J[@]}" $B/webhooks/consumer/Nope
cap5 GET_pact_webhooks "${J[@]}" $B/pacts/provider/Provider/consumer/Consumer/webhooks
cap5 GET_pact_webhooks_status_before "${J[@]}" $B/pacts/provider/Provider/consumer/Consumer/webhooks/status
cap5 GET_pact_webhooks_status_Web "${J[@]}" $B/pacts/provider/Provider/consumer/Web/webhooks/status
cap5 GET_triggered_webhooks_1.0.0_before "${J[@]}" $B/pacts/provider/Provider/consumer/Consumer/version/1.0.0/triggered-webhooks
cap5 OPTIONS_webhooks -X OPTIONS $B/webhooks
cap5 OPTIONS_webhook -X OPTIONS "$B/webhooks/$W_CP"
cap5 OPTIONS_webhook_execute -X OPTIONS "$B/webhooks/$W_CP/execute"

echo "phase 5: test execution"
cli5 test-webhook test-webhook --uuid "$W_CP"
cap5 POST_webhook_execute "${J[@]}" -X POST "$B/webhooks/$W_CP/execute"
cap5 POST_webhook_execute_global "${J[@]}" -X POST "$B/webhooks/$W_GLOBAL/execute"
cap5 POST_webhooks_execute_unsaved "${J[@]}" -X POST -d '{"consumer":{"name":"Consumer"},"provider":{"name":"Provider"},"request":{"method":"POST","url":"'"$HOOK"'/hook/unsaved","body":"'"$(printf '%s' "$BODY" | sed 's/"/\\"/g')"'"}}' $B/webhooks/execute
cap5 POST_webhooks_execute_unknown "${J[@]}" -X POST $B/webhooks/nope/execute
cap5 POST_webhook_execute_failing "${J[@]}" -X POST -d '{"request":{"method":"POST","url":"'"$HOOK_BAD"'/hook"}}' $B/webhooks/execute

echo "phase 5: events on publish and verification"
cli5 publish-changed-3.0.0 publish "$TMP/p3" --consumer-app-version 3.0.0 --branch main --tag dev
sleep 35
cap5 GET_pact_webhooks_status_after_publish "${J[@]}" $B/pacts/provider/Provider/consumer/Consumer/webhooks/status
cap5 GET_triggered_webhooks_3.0.0 "${J[@]}" $B/pacts/provider/Provider/consumer/Consumer/version/3.0.0/triggered-webhooks
i=0; for u in $(python3 -c 'import json; d=json.load(open("'"$F5"'/GET_triggered_webhooks_3.0.0.json")); print(" ".join(t["_links"]["pb:logs"]["href"].split("/")[-2] for t in d["_embedded"]["triggeredWebhooks"]))'); do
  cap5 "GET_triggered_webhook_logs_3.0.0_$i" "${J[@]}" "$B/triggered-webhooks/$u/logs"; i=$((i+1)); done
cap5 GET_triggered_webhook_logs_unknown "${J[@]}" "$B/triggered-webhooks/nope/logs"
cli5 publish-same-3.0.1 publish "$TMP/p3" --consumer-app-version 3.0.1 --branch main
sleep 30
cap5 GET_triggered_webhooks_3.0.1 "${J[@]}" $B/pacts/provider/Provider/consumer/Consumer/version/3.0.1/triggered-webhooks
cap5 GET_pact_webhooks_status_after_republish "${J[@]}" $B/pacts/provider/Provider/consumer/Consumer/webhooks/status
BROKER=$B PROVIDER_VERSION=2 PROVIDER_BRANCH=main SELECTORS='[{"branch":"main"}]' node --test "$ROOT/e2e/provider.test.js" >/dev/null 2>&1 || true
sleep 30
VR=$(curl -s "$B/pacts/provider/Provider/consumer/Consumer/latest/verification-results/latest" | python3 -c 'import json,sys; d=json.load(sys.stdin); print(d["_links"]["self"]["href"])')
cap5 GET_verification_triggered_webhooks_success "${J[@]}" "$VR/triggered-webhooks"
i=0; for u in $(python3 -c 'import json; d=json.load(open("'"$F5"'/GET_verification_triggered_webhooks_success.json")); print(" ".join(t["_links"]["pb:logs"]["href"].split("/")[-2] for t in d["_embedded"]["triggeredWebhooks"]))'); do
  cap5 "GET_verification_triggered_webhook_logs_$i" "${J[@]}" "$B/triggered-webhooks/$u/logs"; i=$((i+1)); done
BROKER=$B PROVIDER_VERSION=3 PROVIDER_BRANCH=main FAIL=true SELECTORS='[{"branch":"main"}]' node --test "$ROOT/e2e/provider.test.js" >/dev/null 2>&1 || true
sleep 30
VR=$(curl -s "$B/pacts/provider/Provider/consumer/Consumer/latest/verification-results/latest" | python3 -c 'import json,sys; d=json.load(sys.stdin); print(d["_links"]["self"]["href"])')
cap5 GET_verification_triggered_webhooks_failure "${J[@]}" "$VR/triggered-webhooks"
cap5 GET_pact_webhooks_status_after_verification "${J[@]}" $B/pacts/provider/Provider/consumer/Consumer/webhooks/status

echo "phase 5: a failing receiver is retried"
cap5 POST_webhooks_failing "${J[@]}" -X POST -d '{"consumer":{"name":"Consumer"},"provider":{"name":"Provider"},"events":[{"name":"contract_published"}],"request":{"method":"POST","url":"'"$HOOK_BAD"'/hook"}}' $B/webhooks
W_BAD=$(python3 -c 'import json; print(json.load(open("'"$F5"'/POST_webhooks_failing.json"))["uuid"])')
cli5 publish-same-3.0.2 publish "$TMP/p3" --consumer-app-version 3.0.2 --branch main
sleep 30
cap5 GET_triggered_webhooks_3.0.2 "${J[@]}" $B/pacts/provider/Provider/consumer/Consumer/version/3.0.2/triggered-webhooks
u=$(python3 -c 'import json; d=json.load(open("'"$F5"'/GET_triggered_webhooks_3.0.2.json")); print([t["_links"]["pb:logs"]["href"].split("/")[-2] for t in d["_embedded"]["triggeredWebhooks"] if t["status"]!="success"][0])')
cap5 GET_triggered_webhook_logs_failing "${J[@]}" "$B/triggered-webhooks/$u/logs"
cap5 GET_pact_webhooks_status_retrying "${J[@]}" $B/pacts/provider/Provider/consumer/Consumer/webhooks/status

echo "phase 5: publish notices without a matching webhook"
mkdir -p "$TMP/other"; python3 - "$PACT" "$TMP/other/other-otherp.json" <<'PY'
import json, sys
d = json.load(open(sys.argv[1])); d["consumer"]["name"] = "Other"; d["provider"]["name"] = "OtherP"
json.dump(d, open(sys.argv[2], "w"), indent=2)
PY
cli5 publish-no-webhooks publish "$TMP/other" --consumer-app-version 1 --branch main
cap5 POST_webhooks_verification_only "${J[@]}" -X POST -d '{"provider":{"name":"OtherP"},"events":[{"name":"provider_verification_published"}],"request":{"method":"POST","url":"'"$HOOK"'/hook/otherp"}}' $B/webhooks
cli5 publish-no-webhook-for-event publish "$TMP/other" --consumer-app-version 2 --branch main

echo "phase 5: delete"
cap5 DELETE_webhook -X DELETE "$B/webhooks/$W_BAD"
cap5 GET_webhook_after_delete "${J[@]}" "$B/webhooks/$W_BAD"
cap5 DELETE_webhook_unknown -X DELETE "$B/webhooks/nope"
cap5 GET_webhooks_after_delete "${J[@]}" $B/webhooks
cap5 GET_triggered_webhooks_3.0.2_after_delete "${J[@]}" $B/pacts/provider/Provider/consumer/Consumer/version/3.0.2/triggered-webhooks
kill $RECV_OK $RECV_BAD 2>/dev/null || true; trap 'rm -rf "$TMP"' EXIT
unset PACT_BROKER_WEBHOOK_SCHEME_WHITELIST

echo "done: $(find "$F" "$C" -type f | wc -l | tr -d ' ') files"
