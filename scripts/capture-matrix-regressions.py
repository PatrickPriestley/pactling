#!/usr/bin/env python3
"""Capture matrix verdict regressions from Ruby broker 2.121.2.

Usage: capture-matrix-regressions.py BASE_URL OUTPUT.json
Uses only its Matrix* services/environment; it does not delete broker data.
The projection keeps complete summaries and row identities, excluding volatile HAL.
"""
import itertools
import json
from pathlib import Path
import sys
import urllib.error
import urllib.parse
import urllib.request


def capture(base, output):
    setup, cases, variables = [], [], {}

    def request(method, path, body=None):
        path = path.format(**variables)
        data = None if body is None else json.dumps(body).encode()
        req = urllib.request.Request(
            base + path, data=data, method=method,
            headers={"Content-Type": "application/json", "Accept": "application/hal+json"},
        )
        try:
            response = urllib.request.urlopen(req, timeout=30)
        except urllib.error.HTTPError as error:
            response = error
        with response:
            raw = response.read()
            return response.status, json.loads(raw) if raw else None, response.headers

    status, _, headers = request("GET", "/")
    version = headers.get("X-Pact-Broker-Version")
    assert status == 200 and version == "2.121.2", (status, version)

    def seed(method, path, body, save_uuid=None):
        operation = {"method": method, "path": path, "body": body}
        if save_uuid:
            operation["saveUuid"] = save_uuid
        setup.append(operation)
        status, result, _ = request(method, path, body)
        # A repeated capture may already have its environment. Do not delete it.
        if save_uuid and status == 400:
            _, listing, _ = request("GET", "/environments")
            result = next(e for e in listing["_embedded"]["environments"]
                          if e["name"] == body["name"])
        else:
            assert 200 <= status < 300, (method, path, status, result)
        if save_uuid:
            variables[save_uuid] = result["uuid"]
        return result

    def publish(consumer, provider):
        body = {"consumer": {"name": consumer}, "provider": {"name": provider}, "interactions": []}
        result = seed("PUT", f"/pacts/provider/{provider}/consumer/{consumer}/version/1", body)
        return result["_links"]["pb:publish-verification-results"]["href"].removeprefix(base)

    consumer, provider, web = "MatrixConsumer", "MatrixProvider", "MatrixWeb"
    consumer_result = publish(consumer, provider)
    web_result = publish(web, provider)
    seed("POST", consumer_result, {"providerApplicationVersion": "1", "success": False})
    seed("POST", consumer_result, {"providerApplicationVersion": "2", "success": True})
    seed("POST", web_result, {"providerApplicationVersion": "1", "success": True})
    seed("POST", "/environments", {"name": "matrix-regressions", "production": False}, "environment")
    for number in ["1", "2"]:
        seed("POST", f"/pacticipants/{provider}/versions/{number}/released-versions/environment/{{environment}}", {})
    seed("POST", f"/pacticipants/{consumer}/versions/1/deployed-versions/environment/{{environment}}", {})

    def matrix(name, selectors, ignore=None, environment=False, latest=False):
        params = []
        for participant, number in selectors:
            params.append(("q[][pacticipant]", participant))
            if isinstance(number, dict):
                params.extend((f"q[][{key}]", str(value).lower() if isinstance(value, bool) else value)
                              for key, value in number.items())
            elif number is not None:
                params.append(("q[][version]", number))
        params.append(("latestby", "cvpv"))
        if environment:
            params.append(("environment", "matrix-regressions" if environment is True else environment))
        if latest:
            params.append(("latest", "true"))
        if ignore:
            params.append(("ignore[][pacticipant]", ignore[0]))
            if ignore[1] is not None:
                params.append(("ignore[][version]", ignore[1]))
        path = "/matrix?" + urllib.parse.urlencode(params)
        status, response, _ = request("GET", path)
        assert status == 200, (name, status, response)
        rows = []
        for row in response["matrix"]:
            rows.append({
                "consumer": row["consumer"]["name"],
                "consumerVersion": (row["consumer"]["version"] or {}).get("number"),
                "provider": row["provider"]["name"],
                "providerVersion": (row["provider"]["version"] or {}).get("number"),
                "success": (row["verificationResult"] or {}).get("success"),
                "ignored": row.get("ignored", False),
            })
        cases.append({"name": name, "request": {"method": "GET", "path": path},
                      "status": status, "expected": {"summary": response["summary"], "rows": rows}})

    pair = [(consumer, "1"), (provider, "1")]
    matrix("failed_consumer_first", pair)
    matrix("failed_provider_first", list(reversed(pair)))
    for index, selectors in enumerate(itertools.permutations([(web, "1"), (provider, "1"), (consumer, "1")])):
        matrix(f"three_selectors_{index}", selectors)
    matrix("missing_provider_version", [(web, "1"), (provider, "999")])
    matrix("missing_provider_version_first", [(provider, "999"), (web, "1")])
    matrix("ignore_missing_provider_version", pair, (provider, "999"))
    matrix("ignore_other_existing_provider_version", pair, (provider, "2"))
    matrix("ignore_matching_provider_version", pair, (provider, "1"))
    matrix("ignore_whole_provider", pair, (provider, None))
    matrix("provider_side_ignore_wrong_consumer_version", [(provider, "1")], (consumer, "999"), True)
    matrix("provider_side_ignore_matching_consumer_version", [(provider, "1")], (consumer, "1"), True)
    matrix("released_versions_ignore_only_success", [(consumer, "1")], (provider, "2"), True)
    matrix("released_versions_ignore_only_failure", [(consumer, "1")], (provider, "1"), True)
    matrix("released_versions_ignore_whole_provider", [(consumer, "1")], (provider, None), True)
    publish("MatrixUnverifiedConsumer", "MatrixUnverifiedProvider")
    matrix("unpinned_unverified", [("MatrixUnverifiedConsumer", None), ("MatrixUnverifiedProvider", None)])
    matrix("unpinned_unverified_reverse", [("MatrixUnverifiedProvider", None), ("MatrixUnverifiedConsumer", None)])
    matrix("existing_unverified_pin", [(web, "1"), (provider, "2")])
    matrix("duplicate_selector", [*pair, pair[0]])
    matrix("successful_pin", [(consumer, "1"), (provider, "2")])
    seed("POST", "/environments", {"name": "matrix-single", "production": False}, "singleEnvironment")
    seed("POST", f"/pacticipants/{provider}/versions/1/released-versions/environment/{{singleEnvironment}}", {})
    matrix("single_provider_environment", [(consumer, "1")], environment="matrix-single")
    matrix("unverified_pin_ignored", [(web, "1"), (provider, "2")], (provider, "2"))
    matrix("unverified_latest_version_ignored", [(web, "1")], (provider, "2"), latest=True)
    matrix("any_provider_version_ignore", [(consumer, "1")], (provider, "1"))
    matrix("missing_with_warning", [(consumer, "999")], latest=True)
    matrix("missing_with_failed_integration", [*pair, (web, "999")])
    matrix("ignore_unknown_service", pair, ("NoSuchApp", None))
    matrix("ignore_unknown_service_version", pair, ("NoSuchApp", "1"))
    matrix("latest_provider_unverified", [(web, "1"), (provider, {"latest": True})])
    matrix("latest_provider_success", [(consumer, "1"), (provider, {"latest": True})])
    matrix("latest_provider_success_reverse", [(provider, {"latest": True}), (consumer, "1")])
    matrix("latest_provider_tag_missing", [(web, "1"), (provider, {"latest": True, "tag": "nosuch"})])
    seed("PUT", f"/pacticipants/{provider}/versions/1/tags/prod", {})
    seed("PUT", f"/pacticipants/{provider}/branches/main/versions/1", {})
    for key, value in [("tag", "prod"), ("branch", "main"), ("mainBranch", True)]:
        matrix(f"latest_provider_{key}", [(web, "1"), (provider, {"latest": True, key: value})])
        matrix(f"latest_provider_{key}_reverse", [(provider, {"latest": True, key: value}), (web, "1")])
    matrix("latest_provider_branch_missing", [(web, "1"), (provider, {"latest": True, "branch": "nosuch"})])
    matrix("single_latest_selector_warning", [(web, {"latest": True})])
    output = Path(output)
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps({"brokerVersion": version, "setup": setup, "cases": cases}, indent=2) + "\n")
    print(f"Captured {len(cases)} matrix cases from Ruby {version} to {output}")


if __name__ == "__main__":
    capture(sys.argv[1].rstrip("/"), sys.argv[2])
