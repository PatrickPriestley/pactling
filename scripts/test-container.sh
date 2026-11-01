#!/usr/bin/env bash
# Exercise the packaged server with the real Pact client and a persistent volume.
set -euo pipefail
cd "$(dirname "$0")/.."
image=${1:-pactling:phase6}
docker image inspect "$image" >/dev/null 2>&1 || { echo 'FAIL: release image must exist' >&2; exit 1; }
[ "$(docker image inspect "$image" --format '{{.Config.User}}')" = '65532:65532' ]
[ "$(docker run --rm "$image" --version)" = "pactling $(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)" ]
probe=$(mktemp -d)
name="pactling-probe-$$"
volume="$name-data"
# Keep the disposable container/volume available for diagnosis; remove manually later.
trap 'docker stop "$name" "$name-recreated" >/dev/null 2>&1 || true' EXIT
mkdir -p "$probe/pacts"
PACT_DIR="$probe/pacts" node --test e2e/consumer.test.js >"$probe/consumer.log" 2>&1
docker run -d --name "$name" -p 127.0.0.1::9292 -v "$volume:/data" "$image" >/dev/null
docker cp "$name:/usr/share/licenses/pactling/LICENSE" "$probe/LICENSE"
cmp LICENSE "$probe/LICENSE"
port=$(docker port "$name" 9292/tcp | sed 's/.*://')
url="http://127.0.0.1:$port"
for _ in $(seq 1 100); do curl -sf "$url/" >/dev/null && break; sleep .2; done
e2e/node_modules/.bin/pact-broker publish "$probe/pacts" --consumer-app-version 1.0.0 --broker-base-url "$url"
curl -sf "$url/pacts/provider/Provider/consumer/Consumer/version/1.0.0" >"$probe/before.json"
docker restart "$name" >/dev/null
port=$(docker port "$name" 9292/tcp | sed 's/.*://')
url="http://127.0.0.1:$port"
for _ in $(seq 1 100); do curl -sf "$url/" >/dev/null && break; sleep .2; done
curl -sf "$url/pacts/provider/Provider/consumer/Consumer/version/1.0.0" >"$probe/after.json"
python3 - "$probe/after.json" <<'PY'
import json, sys
pact = json.load(open(sys.argv[1]))
assert pact['consumer']['name'] == 'Consumer'
assert len(pact['interactions']) == 1
PY
docker stop "$name" >/dev/null
docker run -d --name "$name-recreated" -p 127.0.0.1::9292 -v "$volume:/data" "$image" >/dev/null
port=$(docker port "$name-recreated" 9292/tcp | sed 's/.*://')
url="http://127.0.0.1:$port"
for _ in $(seq 1 100); do curl -sf "$url/" >/dev/null && break; sleep .2; done
curl -sf "$url/pacts/provider/Provider/consumer/Consumer/version/1.0.0" >/dev/null
docker cp "$name-recreated:/data/pactling.db" "$probe/pactling.db"
[ -s "$probe/pactling.db" ]
echo "PASS: non-root image, CLI publish, restart persistence in /data ($probe; $volume)"
