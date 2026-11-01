#!/usr/bin/env python3
"""Seed a reproducible dataset into a Pact broker over HTTP and measure the read and
write paths that matter for a deployment gate (issue #6).

    scripts/bench.py BASE_URL --consumers 20 --providers 5 --versions 50 --out results.json

Dataset: every consumer has a pact with every provider; each consumer has `versions`
versions on main (the newest tagged `prod`) whose pact content changes every 5 versions;
each provider has `versions` versions on main, the last `--verified-tail` of which verify
every latest pact (every 7th provider version fails); one environment holds each
provider's newest version as deployed and the previous one as released, and each
consumer's newest version is deployed as two application instances. Works against
pactling and the Ruby broker alike. Timings are wall-clock per request from this process.
"""
import argparse
import base64
import concurrent.futures
import json
import statistics
import time
import urllib.error
import urllib.parse
import urllib.request

PACT = {"consumer": {"name": "C"}, "provider": {"name": "P"}, "interactions": [
    {"description": "a request", "request": {"method": "GET", "path": "/thing"},
     "response": {"status": 200, "body": {"id": 1}}}], "metadata": {"pactSpecification": {"version": "2.0.0"}}}


def request(base, method, path, body=None):
    data = None if body is None else json.dumps(body).encode()
    headers = {"Accept": "application/hal+json"}
    if body is not None:
        headers["Content-Type"] = "application/json"
    req = urllib.request.Request(base + path, data=data, method=method, headers=headers)
    try:
        with urllib.request.urlopen(req, timeout=120) as r:
            raw = r.read()
            return r.status, (json.loads(raw) if raw else None)
    except urllib.error.HTTPError as e:
        raw = e.read()
        return e.code, (json.loads(raw) if raw else None)


def timed(fn):
    t = time.perf_counter()
    result = fn()
    return (time.perf_counter() - t) * 1000, result


def percentiles(samples):
    s = sorted(samples)
    pick = lambda q: s[min(len(s) - 1, int(round(q * (len(s) - 1))))]
    return {"n": len(s), "p50_ms": round(pick(0.5), 2), "p95_ms": round(pick(0.95), 2),
            "max_ms": round(s[-1], 2), "mean_ms": round(statistics.fmean(s), 2)}


def contract(consumer, provider, salt):
    content = json.loads(json.dumps(PACT))
    content["consumer"]["name"], content["provider"]["name"] = consumer, provider
    content["interactions"][0]["response"]["body"]["salt"] = salt
    return {"consumerName": consumer, "providerName": provider, "specification": "pact",
            "contentType": "application/json",
            "content": base64.b64encode(json.dumps(content).encode()).decode()}


def path_of(href):
    return urllib.parse.urlparse(href).path


def seed(base, consumers, providers, versions, verified_tail):
    times = {"publish": [], "verify": [], "deploy": []}
    status, env = request(base, "POST", "/environments", {"name": "bench-test", "production": False})
    assert status in (200, 201), (status, env)
    env_uuid = env["uuid"]
    cs = [f"BenchConsumer{i:03d}" for i in range(consumers)]
    ps = [f"BenchProvider{j:03d}" for j in range(providers)]
    for v in range(1, versions + 1):
        for c in cs:
            body = {"pacticipantName": c, "pacticipantVersionNumber": f"{v}.0.0", "branch": "main",
                    "tags": ["prod"] if v == versions else [], "contracts": [contract(c, p, v // 5) for p in ps]}
            ms, (status, _) = timed(lambda: request(base, "POST", "/contracts/publish", body))
            assert status == 200, status
            times["publish"].append(ms)
    for p in ps:
        for v in range(1, versions + 1):
            request(base, "PUT", f"/pacticipants/{p}/branches/main/versions/{v}", {})
        for v in range(max(1, versions - verified_tail + 1), versions + 1):
            for c in cs:
                status, pact = request(base, "GET", f"/pacts/provider/{p}/consumer/{c}/latest")
                path = path_of(pact["_links"]["pb:publish-verification-results"]["href"])
                ms, (status, _) = timed(lambda: request(base, "POST", path, {"providerApplicationVersion": str(v), "success": v % 7 != 0}))
                assert status == 201, status
                times["verify"].append(ms)
        ms, (status, _) = timed(lambda: request(base, "POST", f"/pacticipants/{p}/versions/{versions}/deployed-versions/environment/{env_uuid}", {}))
        assert status in (200, 201), status
        times["deploy"].append(ms)
        request(base, "POST", f"/pacticipants/{p}/versions/{max(1, versions - 1)}/released-versions/environment/{env_uuid}", {})
    for c in cs:
        for instance in ("blue", "green"):
            ms, _ = timed(lambda: request(base, "POST", f"/pacticipants/{c}/versions/{versions}.0.0/deployed-versions/environment/{env_uuid}", {"applicationInstance": instance}))
            times["deploy"].append(ms)
    return env_uuid, cs, ps, times


def matrix_path(selectors, **extra):
    params = []
    for name, version in selectors:
        params.append(("q[][pacticipant]", name))
        if version:
            params.append(("q[][version]", version))
    params.extend(extra.items())
    return "/matrix?" + urllib.parse.urlencode(params)


def measure_reads(base, cs, ps, versions, rounds):
    reads = {
        "can-i-deploy consumer to environment": lambda i: matrix_path([(cs[i % len(cs)], f"{versions}.0.0")], latestby="cvp", environment="bench-test"),
        "can-i-deploy consumer to tag": lambda i: matrix_path([(cs[i % len(cs)], f"{versions}.0.0")], latestby="cvp", latest="true", tag="prod"),
        "can-i-deploy provider to environment": lambda i: matrix_path([(ps[i % len(ps)], str(versions))], latestby="cvp", environment="bench-test"),
        "matrix integration (all rows)": lambda i: f"/matrix/provider/{ps[i % len(ps)]}/consumer/{cs[i % len(cs)]}",
        "pacts for verification (provider, defaults)": lambda i: f"/pacts/provider/{ps[i % len(ps)]}/for-verification",
        "latest pacts list": lambda i: "/pacts/latest",
    }
    out = {}
    for name, path in reads.items():
        samples, last = [], None
        for i in range(rounds):
            ms, (status, doc) = timed(lambda: request(base, "GET", path(i)))
            assert status == 200, (name, status, doc)
            samples.append(ms)
            last = doc
        out[name] = percentiles(samples)
        if "matrix" in (last or {}):
            out[name]["rows"] = len(last["matrix"])
            out[name]["deployable"] = last["summary"]["deployable"]
    return out


def measure_contention(base, cs, ps, versions, workers, seconds):
    """Publish, verify, deploy and ask can-i-deploy at once; count errors."""
    status, envs = request(base, "GET", "/environments")
    env_uuid = next(e["uuid"] for e in envs["_embedded"]["environments"] if e["name"] == "bench-test")
    stop = time.perf_counter() + seconds
    latencies = {"publish": [], "verify": [], "deploy": [], "can-i-deploy": []}
    outcome = {"ok": 0, "error": 0, "statuses": {}}

    def worker(k):
        n = 0
        while time.perf_counter() < stop:
            n += 1
            c, p = cs[(k + n) % len(cs)], ps[(k + n) % len(ps)]
            kind = ["publish", "verify", "deploy", "can-i-deploy"][n % 4]
            if kind == "publish":
                body = {"pacticipantName": c, "pacticipantVersionNumber": f"{versions}.{k}.{n}", "branch": "main",
                        "contracts": [contract(c, q, 10_000 + n) for q in ps]}
                ms, (status, _) = timed(lambda: request(base, "POST", "/contracts/publish", body))
                want = (200,)
            elif kind == "verify":
                _, pact = request(base, "GET", f"/pacts/provider/{p}/consumer/{c}/latest")
                path = path_of(pact["_links"]["pb:publish-verification-results"]["href"])
                ms, (status, _) = timed(lambda: request(base, "POST", path, {"providerApplicationVersion": f"{versions}-{k}-{n}", "success": True}))
                want = (201,)
            elif kind == "deploy":
                ms, (status, _) = timed(lambda: request(base, "POST", f"/pacticipants/{c}/versions/{versions}.0.0/deployed-versions/environment/{env_uuid}", {"applicationInstance": f"w{k}"}))
                want = (200, 201)
            else:
                ms, (status, _) = timed(lambda: request(base, "GET", matrix_path([(c, f"{versions}.0.0")], latestby="cvp", environment="bench-test")))
                want = (200,)
            latencies[kind].append(ms)
            outcome["statuses"][str(status)] = outcome["statuses"].get(str(status), 0) + 1
            outcome["ok" if status in want else "error"] += 1

    with concurrent.futures.ThreadPoolExecutor(max_workers=workers) as pool:
        list(pool.map(worker, range(workers)))
    total = outcome["ok"] + outcome["error"]
    return {"workers": workers, "seconds": seconds, "requests": total, "throughput_rps": round(total / seconds, 1),
            "errors": outcome["error"], "statuses": outcome["statuses"],
            **{f"{k} latency": percentiles(v) for k, v in latencies.items() if v}}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("base")
    ap.add_argument("--consumers", type=int, default=20)
    ap.add_argument("--providers", type=int, default=5)
    ap.add_argument("--versions", type=int, default=50)
    ap.add_argument("--verified-tail", type=int, default=3)
    ap.add_argument("--rounds", type=int, default=30)
    ap.add_argument("--workers", type=int, default=8)
    ap.add_argument("--seconds", type=int, default=15)
    ap.add_argument("--out")
    a = ap.parse_args()
    base = a.base.rstrip("/")
    assert request(base, "GET", "/")[0] == 200
    t0 = time.perf_counter()
    _, cs, ps, seed_times = seed(base, a.consumers, a.providers, a.versions, a.verified_tail)
    result = {
        "broker": base,
        "dataset": {"consumers": a.consumers, "providers": a.providers, "integrations": a.consumers * a.providers,
                    "consumer_versions": a.consumers * a.versions, "provider_versions": a.providers * a.versions,
                    "pact_publications": a.consumers * a.providers * a.versions,
                    "verifications": a.consumers * a.providers * a.verified_tail,
                    "seed_seconds": round(time.perf_counter() - t0, 1)},
        "seed": {k: percentiles(v) for k, v in seed_times.items() if v},
        "reads": measure_reads(base, cs, ps, a.versions, a.rounds),
        "contention": measure_contention(base, cs, ps, a.versions, a.workers, a.seconds),
    }
    text = json.dumps(result, indent=2)
    if a.out:
        with open(a.out, "w") as f:
            f.write(text + "\n")
    print(text)


if __name__ == "__main__":
    main()
