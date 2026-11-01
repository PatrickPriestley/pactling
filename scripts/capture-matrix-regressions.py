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

    def publish(consumer, provider, number="1", interactions=None):
        body = {"consumer": {"name": consumer}, "provider": {"name": provider}, "interactions": interactions or []}
        result = seed("PUT", f"/pacts/provider/{provider}/consumer/{consumer}/version/{number}", body)
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

    def matrix(name, selectors, ignore=None, environment=False, latest=False, route=None, tag=None, latestby="cvpv"):
        params = []
        for participant, number in selectors:
            params.append(("q[][pacticipant]", participant))
            if isinstance(number, dict):
                params.extend((f"q[][{key}]", str(value).lower() if isinstance(value, bool) else value)
                              for key, value in number.items())
            elif number is not None:
                params.append(("q[][version]", number))
        if latestby is not None:
            params.append(("latestby", latestby))
        if environment:
            params.append(("environment", "matrix-regressions" if environment is True else environment))
        if latest:
            params.append(("latest", "true"))
        if tag:
            params.append(("tag", tag))
        for ignored in ([ignore] if isinstance(ignore, tuple) else ignore or []):
            params.append(("ignore[][pacticipant]", ignored[0]))
            if ignored[1] is not None:
                params.append(("ignore[][version]", ignored[1]))
        path = route or "/matrix?" + urllib.parse.urlencode(params)
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
    forward = publish("MatrixDuplexA", "MatrixDuplexB")
    backward = publish("MatrixDuplexB", "MatrixDuplexA")
    seed("POST", forward, {"providerApplicationVersion": "1", "success": True})
    seed("POST", backward, {"providerApplicationVersion": "1", "success": False})
    matrix("directional_integration", [], route="/matrix/provider/MatrixDuplexB/consumer/MatrixDuplexA?latestby=cvpv")
    matrix("bidirectional_selectors", [("MatrixDuplexA", "1"), ("MatrixDuplexB", "1")])
    matrix("two_missing_providers", [(consumer, "1"), (provider, "998"), (provider, "999")])
    matrix("two_missing_services", [(consumer, "1"), (provider, "999"), (web, "999")])
    matrix("latest_consumer_branch_missing", [(consumer, {"latest": True, "branch": "absent"}), (provider, "1")])
    # Review findings on the first capture: verdict precedence, notice order, the
    # main-branch missing notice and tag/branch selectors without latest=true.
    oc, oa, ob, tp = "MatrixOrderC", "MatrixOrderA", "MatrixOrderB", "MatrixTagP"
    order_a, order_b, tag_p = publish(oc, oa), publish(oc, ob), publish(oc, tp)
    seed("PUT", f"/pacticipants/{oa}/versions/1", {})
    seed("POST", order_b, {"providerApplicationVersion": "1", "success": False})
    seed("POST", tag_p, {"providerApplicationVersion": "1", "success": True})
    seed("PUT", f"/pacticipants/{tp}/versions/2", {})
    for number in ["1", "2"]:
        seed("PUT", f"/pacticipants/{tp}/versions/{number}/tags/both", {})
        seed("PUT", f"/pacticipants/{tp}/branches/main/versions/{number}", {})
    seed("PUT", f"/pacticipants/{tp}/versions/2/tags/unverified", {})
    seed("PUT", f"/pacticipants/{oc}/versions/1/tags/ct", {})
    matrix("unknown_and_failed_is_null", [(oc, "1"), (ob, "1"), (oa, "1")])
    matrix("unknown_and_failed_single_selector", [(oc, "1")])
    matrix("missing_selectors_keep_query_order", [(oc, "1"), (ob, "999"), (oa, "999")])
    matrix("ignore_warnings_keep_query_order", [(oc, "1")], [(ob, "9"), ("NoSuchApp", None)])
    matrix("main_branch_missing", [(oc, "1"), (oa, {"latest": True, "mainBranch": True})])
    matrix("main_branch_missing_reverse", [(oa, {"latest": True, "mainBranch": True}), (oc, "1")])
    matrix("latest_without_versions", [("MatrixUnverifiedConsumer", "1"), ("MatrixUnverifiedProvider", {"latest": True})])
    matrix("tag_without_latest_missing", [(oc, "1"), (oa, {"tag": "nosuch"})])
    matrix("tag_without_latest_missing_reverse", [(oa, {"tag": "nosuch"}), (oc, "1")])
    matrix("tag_without_latest_missing_first_of_two", [(oa, {"tag": "nosuch"}), (ob, "999"), (oc, "1")])
    matrix("missing_selector_without_integration_keeps_order", [(oc, "1"), (web, "999"), (oa, "999")])
    matrix("tag_without_latest_verified", [(oc, "1"), (tp, {"tag": "both"})])
    matrix("tag_without_latest_verified_reverse", [(tp, {"tag": "both"}), (oc, "1")])
    matrix("tag_without_latest_unverified", [(oc, "1"), (tp, {"tag": "unverified"})])
    matrix("branch_without_latest", [(oc, "1"), (tp, {"branch": "main"})])
    matrix("branch_without_latest_missing", [(oc, "1"), (tp, {"branch": "nosuch"})])
    matrix("main_branch_without_latest", [(oc, "1"), (tp, {"mainBranch": True})])
    matrix("main_branch_without_latest_missing", [(oc, "1"), (oa, {"mainBranch": True})])
    matrix("consumer_tag_without_latest", [(oc, {"tag": "ct"}), (tp, "1")])
    matrix("consumer_tag_without_latest_single", [(oc, {"tag": "ct"})])
    matrix("two_latest_selectors_warning", [(oc, {"latest": True}), (ob, {"latest": True})])
    matrix("to_tag_option_no_environment_warning", [(oc, "1")], latest=True, tag="prod")
    # Environment selection with two released provider versions of which one is verified.
    seed("POST", "/environments", {"name": "matrix-partial", "production": False}, "partialEnvironment")
    for number in ["1", "2"]:
        seed("POST", f"/pacticipants/{tp}/versions/{number}/released-versions/environment/{{partialEnvironment}}", {})
    matrix("environment_with_unverified_release", [(oc, "1")], environment="matrix-partial")
    matrix("environment_with_unverified_release_ignored", [(oc, "1")], (tp, "2"), environment="matrix-partial")
    # #23: an older failed release and newer successful deployment coexist. Version
    # numbers deliberately disagree with creation order: 2 was created after 99.
    lc, lp, lw = "MatrixLatestConsumer", "MatrixLatestProvider", "MatrixLatestWeb"
    result = publish(lc, lp)
    seed("POST", result, {"providerApplicationVersion": "99", "success": False})
    seed("POST", result, {"providerApplicationVersion": "2", "success": True})
    web_result = publish(lw, lp)
    seed("POST", web_result, {"providerApplicationVersion": "99", "success": True})
    seed("POST", web_result, {"providerApplicationVersion": "2", "success": True})
    seed("POST", "/environments", {"name": "matrix-latestby", "production": False}, "latestEnvironment")
    seed("POST", f"/pacticipants/{lp}/versions/99/released-versions/environment/{{latestEnvironment}}", {})
    seed("POST", f"/pacticipants/{lp}/versions/2/deployed-versions/environment/{{latestEnvironment}}", {})
    for name in [lc, lw]:
        seed("POST", f"/pacticipants/{name}/versions/1/deployed-versions/environment/{{latestEnvironment}}", {})
    for mode in ["cvp", "cvpv", "cp", None]:
        suffix = mode or "none"
        matrix(f"latestby_{suffix}_consumer_environment", [(lc, "1")], environment="matrix-latestby", latestby=mode)
        matrix(f"latestby_{suffix}_provider_environment", [(lp, None)], environment="matrix-latestby", latestby=mode)
        matrix(f"latestby_{suffix}_two_selectors", [(lc, "1"), (lp, None)], latestby=mode)
        matrix(f"latestby_{suffix}_two_selectors_reverse", [(lp, None), (lc, "1")], latestby=mode)
    matrix("latestby_cvp_ignore_new_provider", [(lc, "1")], (lp, "2"), environment="matrix-latestby", latestby="cvp")
    matrix("latestby_cvp_ignore_old_provider", [(lc, "1")], (lp, "99"), environment="matrix-latestby", latestby="cvp")
    # cp spans consumer versions; equal provider-order ties follow the Ruby query
    # order rather than choosing the newest consumer version by assumption.
    lc = "MatrixLatestMultiConsumer"
    result = publish(lc, lp)
    seed("POST", result, {"providerApplicationVersion": "99", "success": False})
    seed("POST", result, {"providerApplicationVersion": "2", "success": True})
    publish(lc, lp, "2")
    seed("POST", "/environments", {"name": "matrix-latestby-multiple", "production": False}, "multiEnvironment")
    seed("POST", f"/pacticipants/{lp}/versions/99/released-versions/environment/{{multiEnvironment}}", {})
    seed("POST", f"/pacticipants/{lp}/versions/2/deployed-versions/environment/{{multiEnvironment}}", {})
    seed("POST", f"/pacticipants/{lc}/versions/1/deployed-versions/environment/{{multiEnvironment}}", {})
    seed("POST", f"/pacticipants/{lc}/versions/2/released-versions/environment/{{multiEnvironment}}", {})
    for mode in ["cvp", "cvpv", "cp", None]:
        suffix = mode or "none"
        matrix(f"latestby_{suffix}_multiple_consumers", [(lc, None)], environment="matrix-latestby-multiple", latestby=mode)
        matrix(f"latestby_{suffix}_all_two_selectors", [(lc, None), (lp, None)], latestby=mode)
    # Explicit consumer selectors in opposite orders must not change cp tie-breaking.
    for mode in ["cvp", "cp"]:
        matrix(f"latestby_{mode}_split_consumer_selectors", [(lc, "1"), (lp, None), (lc, "2")], latestby=mode)
        matrix(f"latestby_{mode}_split_consumer_selectors_reverse", [(lc, "2"), (lp, None), (lc, "1")], latestby=mode)
    split = "MatrixLatestSplitConsumer"
    for name, success in [("MatrixLatestSplitA", True), ("MatrixLatestSplitB", False)]:
        verification = publish(split, name)
        seed("POST", verification, {"providerApplicationVersion": "1", "success": success})
    matrix("latestby_cp_keeps_distinct_providers", [(split, "1")], latest=True, latestby="cp")
    # A new, unverified publication sorts before the older verified publications.
    lc = "MatrixLatestUnknownConsumer"
    result = publish(lc, lp)
    seed("POST", result, {"providerApplicationVersion": "99", "success": False})
    seed("POST", result, {"providerApplicationVersion": "2", "success": True})
    publish(lc, lp, "3", [{"description": "unverified", "request": {"method": "GET", "path": "/latestby"}, "response": {"status": 200}}])
    for mode in ["cvp", "cvpv", "cp", None]:
        matrix(f"latestby_{mode or 'none'}_unverified_first", [(lc, None), (lp, None)], latestby=mode)

    output = Path(output)
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps({"brokerVersion": version, "setup": setup, "cases": cases}, indent=2) + "\n")
    print(f"Captured {len(cases)} matrix cases from Ruby {version} to {output}")


if __name__ == "__main__":
    capture(sys.argv[1].rstrip("/"), sys.argv[2])
