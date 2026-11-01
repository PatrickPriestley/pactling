//! Pacts for verification and verification results, compared with the phase 3 fixtures
//! captured from Ruby broker 2.121.2. The capture's seed and the pact-js verifier's
//! requests are replayed first.
mod common;
use base64::Engine;
use common::{fixture, stable};
use reqwest::StatusCode;
use serde_json::{Value, json};

const HAL: &str = "application/hal+json;charset=utf-8";
const JSON: &str = "application/json;charset=utf-8";
const SHA1: &str = "0350e2d94a510324ee31cd7375bf1660e5d8c485";

fn pact(consumer: &str, mutate: impl Fn(&mut Value)) -> Value {
    let mut p: Value =
        serde_json::from_str(include_str!("fixtures/pact/consumer-provider.json")).unwrap();
    p["consumer"]["name"] = json!(consumer);
    mutate(&mut p);
    p
}

fn contract(consumer: &str, content: &Value) -> Value {
    json!({
        "consumerName": consumer, "providerName": "Provider", "specification": "pact", "contentType": "application/json",
        "content": base64::engine::general_purpose::STANDARD.encode(serde_json::to_vec(content).unwrap())
    })
}

async fn publish(
    s: &common::TestServer,
    consumer: &str,
    version: &str,
    branch: &str,
    tags: Vec<&str>,
    content: &Value,
) {
    let body = json!({ "pacticipantName": consumer, "pacticipantVersionNumber": version, "branch": branch, "tags": tags, "contracts": [contract(consumer, content)] });
    let r = s
        .client
        .post(format!("{}/contracts/publish", s.base_url))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert!(r.status().is_success(), "{}", r.status());
}

async fn post(s: &common::TestServer, path: &str, body: &Value) -> reqwest::Response {
    s.client
        .post(format!("{}{path}", s.base_url))
        .header("content-type", "application/json")
        .header("accept", "application/hal+json")
        .json(body)
        .send()
        .await
        .unwrap()
}

async fn get(s: &common::TestServer, path: &str) -> reqwest::Response {
    s.client
        .get(format!("{}{path}", s.base_url))
        .header("accept", "application/hal+json")
        .send()
        .await
        .unwrap()
}

fn phase3(name: &str, base: &str) -> Value {
    normalise(stable(fixture(&format!("phase3/{name}"), base)))
}

/// Strips what legitimately differs between brokers: link metadata (internal ids) and
/// verification numbers (a broker-wide counter).
fn normalise(v: Value) -> Value {
    let text = serde_json::to_string(&v).unwrap();
    let text = regex::Regex::new(r"/metadata/[A-Za-z0-9_=-]+")
        .unwrap()
        .replace_all(&text, "/metadata/M");
    let text = regex::Regex::new(r"verification-results/\d+")
        .unwrap()
        .replace_all(&text, "verification-results/N");
    let text = regex::Regex::new(r"Verification result \d+ for")
        .unwrap()
        .replace_all(&text, "Verification result N for");
    let text = regex::Regex::new(r#""verificationDate":"[^"]*""#)
        .unwrap()
        .replace_all(&text, r#""verificationDate":"T""#);
    serde_json::from_str(&text).unwrap()
}

async fn assert_matches(
    s: &common::TestServer,
    r: reqwest::Response,
    name: &str,
    status: StatusCode,
) {
    assert_eq!(r.status(), status, "{name}");
    let got: Value = r.json().await.unwrap();
    assert_eq!(normalise(stable(got)), phase3(name, &s.base_url), "{name}");
}

fn ok(version: &str) -> Value {
    json!({"providerApplicationVersion": version, "success": true,
        "testResults": [{"interactionId": "72e9b4bb90f5cb675f1cea3e426b11616e6703a0", "success": true}, {"interactionId": "8da86afba68368b3a91327afde5a9c6b59c02274", "success": true}],
        "verifiedBy": {"clientLanguage": {"name": "pact-core", "testFramework": "pact_ffi", "version": "15.2.1"}, "implementation": "Pact-Rust", "version": "1.2.4"}})
}

fn failed(version: &str) -> Value {
    json!({"providerApplicationVersion": version, "success": false,
        "testResults": [{"interactionDescription": "a request for an order", "interactionId": "72e9b4bb90f5cb675f1cea3e426b11616e6703a0", "mismatches": [{"attribute": "status", "description": "expected 200 but was 500"}], "success": false}, {"interactionDescription": "a request for a missing order", "interactionId": "8da86afba68368b3a91327afde5a9c6b59c02274", "success": true}],
        "verifiedBy": {"clientLanguage": {"name": "pact-core", "testFramework": "pact_ffi", "version": "15.2.1"}, "implementation": "Pact-Rust", "version": "1.2.4"}})
}

/// The capture's seed and verifier runs, in order; results are posted through each
/// pact document's own publish link, as the verifier does.
async fn replay_impl(s: &common::TestServer) {
    publish(
        s,
        "Consumer",
        "1.0.0",
        "main",
        vec!["dev"],
        &pact("Consumer", |_| {}),
    )
    .await;
    publish(
        s,
        "Consumer",
        "2.0.0",
        "feat",
        vec![],
        &pact("Consumer", |p| {
            p["interactions"][0]["response"]["body"]["status"] = json!("closed")
        }),
    )
    .await;
    publish(s, "Web", "1", "main", vec![], &pact("Web", |_| {})).await;
    publish(
        s,
        "Consumer",
        "4.0.0",
        "zeta",
        vec![],
        &pact("Consumer", |p| {
            p["interactions"][0]["response"]["body"]["id"] = json!(4)
        }),
    )
    .await;
    let run = |version: &'static str, branch: &'static str, posts: Vec<(&'static str, Value)>| async move {
        let r = s
            .client
            .put(format!(
                "{}/pacticipants/Provider/branches/{branch}/versions/{version}",
                s.base_url
            ))
            .header("content-type", "application/json")
            .body("{}")
            .send()
            .await
            .unwrap();
        assert!(r.status().is_success());
        for (consumer_version_path, body) in posts {
            let doc: Value = get(
                s,
                &format!("/pacts/provider/Provider/consumer/{consumer_version_path}"),
            )
            .await
            .json()
            .await
            .unwrap();
            let href = doc["_links"]["pb:publish-verification-results"]["href"]
                .as_str()
                .unwrap()
                .to_owned();
            let r = s
                .client
                .post(href)
                .header("content-type", "application/json")
                .json(&body)
                .send()
                .await
                .unwrap();
            assert_eq!(
                r.status(),
                StatusCode::CREATED,
                "{}",
                r.text().await.unwrap()
            );
        }
    };
    run(
        "1",
        "main",
        vec![
            ("Consumer/version/1.0.0", ok("1")),
            ("Web/version/1", ok("1")),
        ],
    )
    .await;
    run(
        "2",
        "main",
        vec![
            ("Consumer/version/1.0.0", failed("2")),
            ("Web/version/1", failed("2")),
        ],
    )
    .await;
    run(
        "3",
        "feat",
        vec![
            ("Consumer/version/1.0.0", ok("3")),
            ("Consumer/version/2.0.0", failed("3")),
            ("Web/version/1", ok("3")),
        ],
    )
    .await;
    run("6", "feat", vec![("Consumer/version/4.0.0", ok("6"))]).await;
}

#[tokio::test]
async fn for_verification_documents_match_the_ruby_broker() {
    let s = common::start().await;
    replay_impl(&s).await;
    let cases: Vec<(&str, Value)> = vec![
        ("empty", json!({})),
        ("empty_array", json!({"consumerVersionSelectors": []})),
        (
            "latest",
            json!({"consumerVersionSelectors": [{"latest": true}]}),
        ),
        ("tag", json!({"consumerVersionSelectors": [{"tag": "dev"}]})),
        (
            "tag_latest",
            json!({"consumerVersionSelectors": [{"tag": "dev", "latest": true}]}),
        ),
        (
            "tag_nope",
            json!({"consumerVersionSelectors": [{"tag": "nope"}]}),
        ),
        (
            "branch",
            json!({"consumerVersionSelectors": [{"branch": "feat"}]}),
        ),
        (
            "branch_consumer",
            json!({"consumerVersionSelectors": [{"branch": "main", "consumer": "Web"}]}),
        ),
        (
            "mainBranch",
            json!({"consumerVersionSelectors": [{"mainBranch": true}]}),
        ),
        (
            "matchingBranch_feat",
            json!({"consumerVersionSelectors": [{"matchingBranch": true}], "providerVersionBranch": "feat"}),
        ),
        (
            "deployed",
            json!({"consumerVersionSelectors": [{"deployed": true}]}),
        ),
        (
            "released",
            json!({"consumerVersionSelectors": [{"released": true}]}),
        ),
        (
            "deployedOrReleased",
            json!({"consumerVersionSelectors": [{"deployedOrReleased": true}]}),
        ),
        (
            "two_selectors",
            json!({"includePendingStatus": true, "consumerVersionSelectors": [{"mainBranch": true}, {"deployedOrReleased": true}, {"matchingBranch": true}], "providerVersionBranch": "main"}),
        ),
        (
            "pending_from_feat",
            json!({"includePendingStatus": true, "consumerVersionSelectors": [{"branch": "zeta"}], "providerVersionBranch": "feat"}),
        ),
        (
            "pending_from_unknown_branch",
            json!({"includePendingStatus": true, "consumerVersionSelectors": [{"branch": "zeta"}], "providerVersionBranch": "x"}),
        ),
        (
            "pending_from_main",
            json!({"includePendingStatus": true, "consumerVersionSelectors": [{"branch": "zeta"}], "providerVersionBranch": "main"}),
        ),
        (
            "pending_no_branch",
            json!({"includePendingStatus": true, "consumerVersionSelectors": [{"branch": "zeta"}]}),
        ),
        (
            "pending_provider_tags",
            json!({"includePendingStatus": true, "consumerVersionSelectors": [{"mainBranch": true}], "providerVersionTags": ["prod"]}),
        ),
        (
            "pending_off",
            json!({"consumerVersionSelectors": [{"branch": "zeta"}], "providerVersionBranch": "main"}),
        ),
    ];
    for (name, body) in cases {
        let r = post(&s, "/pacts/provider/Provider/for-verification", &body).await;
        assert_eq!(r.headers()["content-type"], HAL, "{name}");
        assert_matches(
            &s,
            r,
            &format!("POST_for-verification_{name}.json"),
            StatusCode::OK,
        )
        .await;
    }
    let r = get(&s, "/pacts/provider/Provider/for-verification").await;
    assert_matches(&s, r, "GET_for-verification.json", StatusCode::OK).await;
}

#[tokio::test]
async fn for_verification_errors_match_the_ruby_broker() {
    let s = common::start().await;
    s.client
        .post(format!("{}/pacticipants", s.base_url))
        .json(&json!({"name": "Provider"}))
        .send()
        .await
        .unwrap();
    for (name, body) in [
        (
            "matchingBranch_no_provider_branch",
            json!({"consumerVersionSelectors": [{"matchingBranch": true}]}),
        ),
        (
            "err_bogus_selector",
            json!({"consumerVersionSelectors": [{"bogus": true}]}),
        ),
        (
            "err_types",
            json!({"consumerVersionSelectors": "x", "includePendingStatus": "yes"}),
        ),
    ] {
        let r = post(&s, "/pacts/provider/Provider/for-verification", &body).await;
        assert_matches(
            &s,
            r,
            &format!("POST_for-verification_{name}.json"),
            StatusCode::BAD_REQUEST,
        )
        .await;
    }
    let r = s
        .client
        .post(format!(
            "{}/pacts/provider/Provider/for-verification",
            s.base_url
        ))
        .header("content-type", "application/json")
        .body("nope")
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::BAD_REQUEST);
    assert!(
        r.json::<Value>().await.unwrap()["error"]
            .as_str()
            .unwrap()
            .starts_with("JSON::ParserError - ")
    );
    let r = post(&s, "/pacts/provider/Nope/for-verification", &json!({})).await;
    assert_matches(
        &s,
        r,
        "POST_for-verification_unknown_provider.json",
        StatusCode::NOT_FOUND,
    )
    .await;
    let r = s
        .client
        .request(
            reqwest::Method::OPTIONS,
            format!("{}/pacts/provider/Provider/for-verification", s.base_url),
        )
        .send()
        .await
        .unwrap();
    assert_eq!(
        r.headers()["access-control-allow-methods"],
        "GET, POST, OPTIONS"
    );
}

#[tokio::test]
async fn verification_result_documents_match_the_ruby_broker() {
    let s = common::start().await;
    replay_impl(&s).await;
    let pv = format!("/pacts/provider/Provider/consumer/Consumer/pact-version/{SHA1}");
    let r = get(&s, &format!("{pv}/verification-results/latest")).await;
    assert_matches(
        &s,
        r,
        "GET_verification-results_latest.json",
        StatusCode::OK,
    )
    .await;
    let r = get(
        &s,
        "/pacts/provider/Provider/consumer/Consumer/version/1.0.0/verification-results/latest",
    )
    .await;
    assert_matches(
        &s,
        r,
        "GET_pact_by_version_verification-results_latest.json",
        StatusCode::OK,
    )
    .await;
    let r = get(
        &s,
        "/verification-results/consumer/Consumer/version/1.0.0/latest",
    )
    .await;
    assert_matches(
        &s,
        r,
        "GET_consumer_version_verification-results_latest.json",
        StatusCode::OK,
    )
    .await;
    for (path, name) in [
        (format!("{pv}/verification-results/999"), "GET_verification-results_999.json"),
        ("/pacts/provider/Provider/consumer/Consumer/pact-version/deadbeef/verification-results/latest".to_owned(), "GET_verification-results_unknown_pact.json"),
        ("/verification-results/consumer/Consumer/version/9.9.9/latest".to_owned(), "GET_consumer_version_verification-results_latest_unknown.json"),
    ] {
        let r = get(&s, &path).await;
        assert_eq!(r.headers()["content-type"], JSON, "{name}");
        assert_matches(&s, r, name, StatusCode::NOT_FOUND).await;
    }
    // A result fetched by its own number.
    let latest: Value = get(&s, &format!("{pv}/verification-results/latest"))
        .await
        .json()
        .await
        .unwrap();
    let href = latest["_links"]["self"]["href"]
        .as_str()
        .unwrap()
        .to_owned();
    let by_number: Value = s
        .client
        .get(&href)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(by_number, latest);

    // Publishing without metadata and without test results.
    let r = post(
        &s,
        &format!("{pv}/verification-results"),
        &json!({"providerApplicationVersion": "7", "success": true}),
    )
    .await;
    assert_eq!(r.status(), StatusCode::CREATED);
    let location = r.headers()["location"].to_str().unwrap().to_owned();
    assert!(
        location.starts_with(&format!("{}{pv}/verification-results/", s.base_url)),
        "{location}"
    );
    assert_matches(
        &s,
        r,
        "POST_verification-results_minimal.json",
        StatusCode::CREATED,
    )
    .await;
    // The successful numeric read, through the Location the POST returned (captured).
    let r = s
        .client
        .get(&location)
        .header("accept", "application/hal+json")
        .send()
        .await
        .unwrap();
    assert_matches(
        &s,
        r,
        "GET_verification-results_by_number.json",
        StatusCode::OK,
    )
    .await;
    let r = get(&s, "/pacticipants/Provider/versions/7").await;
    assert_matches(
        &s,
        r,
        "GET_pacticipants_Provider_versions_7.json",
        StatusCode::OK,
    )
    .await;
    let r = get(&s, "/pacticipants/Provider/versions").await;
    assert_matches(
        &s,
        r,
        "GET_pacticipants_Provider_versions.json",
        StatusCode::OK,
    )
    .await;
    let r = get(
        &s,
        "/pacts/provider/Provider/consumer/Consumer/version/1.0.0",
    )
    .await;
    assert_matches(
        &s,
        r,
        "GET_pact_1.0.0_after_verification.json",
        StatusCode::OK,
    )
    .await;

    for (name, body) in [
        ("err_empty", json!({})),
        (
            "err_types",
            json!({"providerApplicationVersion": "", "success": "yes"}),
        ),
    ] {
        let r = post(&s, &format!("{pv}/verification-results"), &body).await;
        assert_matches(
            &s,
            r,
            &format!("POST_verification-results_{name}.json"),
            StatusCode::BAD_REQUEST,
        )
        .await;
    }
    let r = s
        .client
        .post(format!("{}{pv}/verification-results", s.base_url))
        .header("content-type", "application/json")
        .body("nope")
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::BAD_REQUEST);
    let r = post(
        &s,
        "/pacts/provider/Provider/consumer/Consumer/pact-version/deadbeef/verification-results",
        &json!({"providerApplicationVersion": "7", "success": true}),
    )
    .await;
    assert_matches(
        &s,
        r,
        "POST_verification-results_unknown_pact.json",
        StatusCode::NOT_FOUND,
    )
    .await;
    for (path, allowed) in [
        (format!("{pv}/verification-results"), "POST, OPTIONS"),
        (
            format!("{pv}/verification-results/latest"),
            "GET, OPTIONS, DELETE",
        ),
    ] {
        let r = s
            .client
            .request(reqwest::Method::OPTIONS, format!("{}{path}", s.base_url))
            .send()
            .await
            .unwrap();
        assert_eq!(
            r.headers()["access-control-allow-methods"],
            allowed,
            "{path}"
        );
    }
}

#[tokio::test]
async fn metadata_ties_a_result_to_the_consumer_version_it_was_fetched_for() {
    let s = common::start().await;
    publish(
        &s,
        "Consumer",
        "1.0.0",
        "main",
        vec![],
        &pact("Consumer", |_| {}),
    )
    .await;
    publish(
        &s,
        "Consumer",
        "1.0.1",
        "main",
        vec![],
        &pact("Consumer", |_| {}),
    )
    .await;
    // Same content for both versions; fetching 1.0.0's pact by version gives a plain href,
    // fetching through for-verification carries metadata naming the newest (1.0.1).
    let fv: Value = post(
        &s,
        "/pacts/provider/Provider/for-verification",
        &json!({"consumerVersionSelectors": [{"tag": "dev"}]}),
    )
    .await
    .json()
    .await
    .unwrap();
    assert_eq!(fv["_embedded"]["pacts"], json!([]));
    let fv: Value = post(
        &s,
        "/pacts/provider/Provider/for-verification",
        &json!({"consumerVersionSelectors": [{"mainBranch": true}]}),
    )
    .await
    .json()
    .await
    .unwrap();
    let href = fv["_embedded"]["pacts"][0]["_links"]["self"]["href"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(href.contains("/metadata/"));
    let doc: Value = s
        .client
        .get(&href)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let results_href = doc["_links"]["pb:publish-verification-results"]["href"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(results_href.contains("/metadata/"), "{results_href}");
    let r = s
        .client
        .post(&results_href)
        .header("content-type", "application/json")
        .json(&json!({"providerApplicationVersion": "1", "success": true}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::CREATED);
    let body: Value = r.json().await.unwrap();
    assert_eq!(
        body["_links"]["self"]["name"],
        "Verification result 1 for Pact between Consumer (1.0.1) and Provider"
    );
    assert_eq!(
        body["_links"]["pb:pact-version"]["name"],
        "Pact between Consumer (1.0.1) and Provider"
    );
}

fn decode_metadata(href: &str) -> String {
    let m = href
        .split("/metadata/")
        .nth(1)
        .unwrap()
        .split('/')
        .next()
        .unwrap();
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(m)
        .unwrap();
    String::from_utf8(bytes).unwrap()
}

#[tokio::test]
async fn branch_names_with_query_characters_cannot_corrupt_the_metadata() {
    // Review finding: an unescaped "&" in a branch name would inject a fake cv= pair.
    let s = common::start().await;
    publish(
        &s,
        "Consumer",
        "1.0.0",
        "feat&cv=999",
        vec![],
        &pact("Consumer", |_| {}),
    )
    .await;
    let fv: Value = post(
        &s,
        "/pacts/provider/Provider/for-verification",
        &json!({"consumerVersionSelectors": [{"branch": "feat&cv=999"}]}),
    )
    .await
    .json()
    .await
    .unwrap();
    let href = fv["_embedded"]["pacts"][0]["_links"]["self"]["href"]
        .as_str()
        .unwrap()
        .to_owned();
    let decoded = decode_metadata(&href);
    assert!(
        decoded.contains("feat%26cv%3D999"),
        "branch must be form-encoded: {decoded}"
    );
    let doc: Value = s
        .client
        .get(&href)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let results_href = doc["_links"]["pb:publish-verification-results"]["href"]
        .as_str()
        .unwrap()
        .to_owned();
    let r = s
        .client
        .post(&results_href)
        .header("content-type", "application/json")
        .json(&json!({"providerApplicationVersion": "1", "success": true}))
        .send()
        .await
        .unwrap();
    assert_eq!(
        r.status(),
        StatusCode::CREATED,
        "{}",
        r.text().await.unwrap()
    );
    let body: Value = r.json().await.unwrap();
    assert_eq!(
        body["_links"]["pb:pact-version"]["name"],
        "Pact between Consumer (1.0.0) and Provider"
    );
}

#[tokio::test]
async fn metadata_naming_a_foreign_consumer_version_is_ignored() {
    // Hand-built metadata pointing at a version that does not hold this pact.
    let s = common::start().await;
    publish(
        &s,
        "Consumer",
        "1.0.0",
        "main",
        vec![],
        &pact("Consumer", |_| {}),
    )
    .await;
    publish(
        &s,
        "Other",
        "9",
        "main",
        vec![],
        &pact("Other", |p| {
            p["interactions"][0]["response"]["body"]["id"] = json!(9)
        }),
    )
    .await;
    let other: Value = get(&s, "/pacts/provider/Provider/consumer/Other/latest")
        .await
        .json()
        .await
        .unwrap();
    let other_cv = decode_metadata(
        other["_links"]["pb:publish-verification-results"]["href"]
            .as_str()
            .unwrap(),
    );
    let forged = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&other_cv);
    let r = s.client.post(format!("{}/pacts/provider/Provider/consumer/Consumer/pact-version/{SHA1}/metadata/{forged}/verification-results", s.base_url))
        .header("content-type", "application/json").json(&json!({"providerApplicationVersion": "1", "success": true})).send().await.unwrap();
    assert_eq!(r.status(), StatusCode::CREATED);
    let body: Value = r.json().await.unwrap();
    assert_eq!(
        body["_links"]["pb:pact-version"]["name"], "Pact between Consumer (1.0.0) and Provider",
        "falls back to the pact's own consumer version"
    );
    let bogus = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode("cv=999999");
    let r = s.client.post(format!("{}/pacts/provider/Provider/consumer/Consumer/pact-version/{SHA1}/metadata/{bogus}/verification-results", s.base_url))
        .header("content-type", "application/json").json(&json!({"providerApplicationVersion": "2", "success": true})).send().await.unwrap();
    assert_eq!(
        r.status(),
        StatusCode::CREATED,
        "an unknown version id must not break publishing"
    );
}

#[tokio::test]
async fn each_selector_group_in_the_metadata_names_its_own_consumer_version() {
    // Review finding: identical content on two branches, selected by both.
    let s = common::start().await;
    publish(
        &s,
        "Consumer",
        "A",
        "main",
        vec![],
        &pact("Consumer", |_| {}),
    )
    .await;
    publish(
        &s,
        "Consumer",
        "B",
        "feature",
        vec![],
        &pact("Consumer", |_| {}),
    )
    .await;
    let fv: Value = post(
        &s,
        "/pacts/provider/Provider/for-verification",
        &json!({"consumerVersionSelectors": [{"branch": "main"}, {"branch": "feature"}]}),
    )
    .await
    .json()
    .await
    .unwrap();
    let pacts = fv["_embedded"]["pacts"].as_array().unwrap();
    assert_eq!(pacts.len(), 1, "same content is one pact to verify");
    // Ruby's Selector#<=> orders two branch selectors by branch name, not request order
    // (exact source, pact_broker/pacts/selector.rb#branch_comparison).
    assert_eq!(
        pacts[0]["shortDescription"],
        "latest from branch feature, latest from branch main"
    );
    let decoded = decode_metadata(pacts[0]["_links"]["self"]["href"].as_str().unwrap());
    let a: Value = get(&s, "/pacticipants/Consumer/versions/A")
        .await
        .json()
        .await
        .unwrap();
    let b: Value = get(&s, "/pacticipants/Consumer/versions/B")
        .await
        .json()
        .await
        .unwrap();
    let _ = (a, b);
    let cvs: Vec<&str> = decoded
        .split('&')
        .filter(|p| p.starts_with("s%5B%5D%5Bcv%5D="))
        .map(|p| p.split('=').nth(1).unwrap())
        .collect();
    assert_eq!(cvs.len(), 2);
    assert_ne!(
        cvs[0], cvs[1],
        "each group carries its own consumer version id: {decoded}"
    );
}

#[tokio::test]
async fn deployment_selectors_report_only_their_own_environments() {
    // Review finding: a version deployed to one environment and released in another must
    // not be reported as deployed to the second or released in the first, and the Ruby
    // broker encodes only the consumer version in the metadata of these reasons.
    let s = common::start().await;
    publish(
        &s,
        "Consumer",
        "1",
        "main",
        vec![],
        &pact("Consumer", |_| {}),
    )
    .await;
    let test: Value = post(
        &s,
        "/environments",
        &json!({"name": "test", "production": false}),
    )
    .await
    .json()
    .await
    .unwrap();
    let prod: Value = post(
        &s,
        "/environments",
        &json!({"name": "production", "production": true}),
    )
    .await
    .json()
    .await
    .unwrap();
    post(
        &s,
        &format!(
            "/pacticipants/Consumer/versions/1/deployed-versions/environment/{}",
            test["uuid"].as_str().unwrap()
        ),
        &json!({}),
    )
    .await;
    post(
        &s,
        &format!(
            "/pacticipants/Consumer/versions/1/released-versions/environment/{}",
            prod["uuid"].as_str().unwrap()
        ),
        &json!({}),
    )
    .await;
    for (selectors, short) in [
        (json!([{"deployed": true}]), "deployed to test"),
        (json!([{"released": true}]), "released in production"),
        (
            json!([{"deployedOrReleased": true}]),
            "deployed to test, released in production",
        ),
    ] {
        let fv: Value = post(
            &s,
            "/pacts/provider/Provider/for-verification",
            &json!({"consumerVersionSelectors": selectors}),
        )
        .await
        .json()
        .await
        .unwrap();
        let pacts = fv["_embedded"]["pacts"].as_array().unwrap();
        assert_eq!(pacts.len(), 1, "{selectors}");
        assert_eq!(pacts[0]["shortDescription"], short, "{selectors}");
        let metadata = decode_metadata(pacts[0]["_links"]["self"]["href"].as_str().unwrap());
        let groups = metadata.matches("s%5B%5D%5Bcv%5D=").count();
        assert_eq!(
            metadata.matches("s%5B%5D%5B").count(),
            groups,
            "only cv keys in {metadata}"
        );
    }
    let fv: Value = s
        .client
        .get(format!(
            "{}/pacts/provider/Provider/for-verification",
            s.base_url
        ))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        fv["_embedded"]["pacts"][0]["shortDescription"],
        "deployed to test, released in production, latest from main branch",
        "default selectors"
    );

    // Past deployments and ended support are not reported, and an environment selector
    // names only its own environment even when the version is in several.
    let dv: Value = post(
        &s,
        &format!(
            "/pacticipants/Consumer/versions/1/deployed-versions/environment/{}",
            prod["uuid"].as_str().unwrap()
        ),
        &json!({}),
    )
    .await
    .json()
    .await
    .unwrap();
    let test_dv: Value = get(
        &s,
        &format!(
            "/environments/{}/deployed-versions/currently-deployed",
            test["uuid"].as_str().unwrap()
        ),
    )
    .await
    .json()
    .await
    .unwrap();
    let fv: Value = post(
        &s,
        "/pacts/provider/Provider/for-verification",
        &json!({"consumerVersionSelectors": [{"environment": "test"}]}),
    )
    .await
    .json()
    .await
    .unwrap();
    assert_eq!(fv["_embedded"]["pacts"][0]["shortDescription"], "in test");
    let patch = |path: String, body: Value| {
        let s = &s;
        async move {
            s.client
                .patch(format!("{}{path}", s.base_url))
                .header("content-type", "application/merge-patch+json")
                .json(&body)
                .send()
                .await
                .unwrap()
        }
    };
    let r = patch(
        format!(
            "/deployed-versions/{}",
            test_dv["_embedded"]["deployedVersions"][0]["uuid"]
                .as_str()
                .unwrap()
        ),
        json!({"currentlyDeployed": false}),
    )
    .await;
    assert_eq!(r.status(), StatusCode::OK);
    let rv: Value = get(
        &s,
        &format!(
            "/environments/{}/released-versions/currently-supported",
            prod["uuid"].as_str().unwrap()
        ),
    )
    .await
    .json()
    .await
    .unwrap();
    let r = patch(
        format!(
            "/released-versions/{}",
            rv["_embedded"]["releasedVersions"][0]["uuid"]
                .as_str()
                .unwrap()
        ),
        json!({"currentlySupported": false}),
    )
    .await;
    assert_eq!(r.status(), StatusCode::OK);
    assert_eq!(dv["currentlyDeployed"], true);
    let fv: Value = post(
        &s,
        "/pacts/provider/Provider/for-verification",
        &json!({"consumerVersionSelectors": [{"deployedOrReleased": true}]}),
    )
    .await
    .json()
    .await
    .unwrap();
    assert_eq!(
        fv["_embedded"]["pacts"][0]["shortDescription"], "deployed to production",
        "undeployed and unsupported records are not reasons"
    );
}
