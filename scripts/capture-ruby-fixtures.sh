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
echo "done: $(find "$F" "$C" -type f | wc -l | tr -d ' ') files"
