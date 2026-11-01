//! Environments, deployments, the matrix and can-i-deploy, compared with the phase 4
//! fixtures captured from Ruby broker 2.121.2 by replaying the capture's sequence.
mod common;
use base64::Engine;
use common::{fixture, stable};
use reqwest::StatusCode;
use serde_json::{Value, json};

const HAL: &str = "application/hal+json;charset=utf-8";
const JSON: &str = "application/json;charset=utf-8";

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

async fn get_json(s: &common::TestServer, path: &str) -> Value {
    get(s, path).await.json().await.unwrap()
}

/// Strips what differs between brokers: uuids are removed by `stable`, so here only
/// link metadata, verification numbers and verification dates.
fn normalise(v: Value) -> Value {
    let text = serde_json::to_string(&v).unwrap();
    // Metadata is decoded and kept, with consumer version ids (database ids) normalised,
    // so the selector keys the Ruby broker encodes are compared.
    let text = regex::Regex::new(r"/metadata/([A-Za-z0-9_=-]+)")
        .unwrap()
        .replace_all(&text, |c: &regex::Captures| {
            use base64::Engine;
            let raw = c[1].trim_end_matches('=');
            let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
                .decode(raw)
                .map(|b| String::from_utf8_lossy(&b).into_owned())
                .unwrap_or_else(|_| c[1].to_owned());
            let decoded = regex::Regex::new(r"cv%5D=\d+")
                .unwrap()
                .replace_all(&decoded, "cv%5D=N");
            format!("/metadata/{decoded}")
        });
    let text = regex::Regex::new(r"verification-results/\d+")
        .unwrap()
        .replace_all(&text, "verification-results/N");
    let text = regex::Regex::new(r#""verifiedAt":"[^"]*""#)
        .unwrap()
        .replace_all(&text, r#""verifiedAt":"T""#);
    let text = regex::Regex::new(r#""(undeployedAt|deployed_at|undeployed_at|supportEndedAt|released_at|support_ended_at)":"[^"]*""#).unwrap().replace_all(&text, r#""$1":"T""#);
    // uuids inside hrefs
    let text = regex::Regex::new(r"[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}")
        .unwrap()
        .replace_all(&text, "UUID");
    serde_json::from_str(&text).unwrap()
}

fn phase4(name: &str, base: &str) -> Value {
    normalise(stable(fixture(&format!("phase4/{name}"), base)))
}

async fn assert_matches(
    s: &common::TestServer,
    r: reqwest::Response,
    name: &str,
    status: StatusCode,
) -> Value {
    assert_eq!(r.status(), status, "{name}");
    let got: Value = r.json().await.unwrap();
    let (left, right) = (normalise(stable(got.clone())), phase4(name, &s.base_url));
    // PACTLING_DUMP_DIR=/some/dir writes both documents of a mismatch for diffing.
    if left != right
        && let Ok(dir) = std::env::var("PACTLING_DUMP_DIR")
    {
        std::fs::write(
            format!("{dir}/{name}.got.json"),
            serde_json::to_string_pretty(&left).unwrap(),
        )
        .unwrap();
        std::fs::write(
            format!("{dir}/{name}.want.json"),
            serde_json::to_string_pretty(&right).unwrap(),
        )
        .unwrap();
    }
    assert_eq!(left, right, "{name}");
    got
}

/// Posts a verification result for the pact a consumer version published, as the
/// verifier does (through the pact's own publish link).
async fn verify(
    s: &common::TestServer,
    provider_version: &str,
    branch: &str,
    consumer_version_path: &str,
    success: bool,
) {
    let r = s
        .client
        .put(format!(
            "{}/pacticipants/Provider/branches/{branch}/versions/{provider_version}",
            s.base_url
        ))
        .header("content-type", "application/json")
        .body("{}")
        .send()
        .await
        .unwrap();
    assert!(r.status().is_success());
    let doc = get_json(
        s,
        &format!("/pacts/provider/Provider/consumer/{consumer_version_path}"),
    )
    .await;
    let href = doc["_links"]["pb:publish-verification-results"]["href"]
        .as_str()
        .unwrap()
        .to_owned();
    let body = if success {
        json!({"providerApplicationVersion": provider_version, "success": true, "testResults": [{"interactionId": "72e9b4bb90f5cb675f1cea3e426b11616e6703a0", "success": true}, {"interactionId": "8da86afba68368b3a91327afde5a9c6b59c02274", "success": true}], "verifiedBy": {"clientLanguage": {"name": "pact-core", "testFramework": "pact_ffi", "version": "15.2.1"}, "implementation": "Pact-Rust", "version": "1.2.4"}})
    } else {
        json!({"providerApplicationVersion": provider_version, "success": false, "testResults": [{"interactionDescription": "a request for an order", "interactionId": "72e9b4bb90f5cb675f1cea3e426b11616e6703a0", "mismatches": [{"attribute": "status", "description": "expected 200 but was 500"}], "success": false}, {"interactionDescription": "a request for a missing order", "interactionId": "8da86afba68368b3a91327afde5a9c6b59c02274", "success": true}], "verifiedBy": {"clientLanguage": {"name": "pact-core", "testFramework": "pact_ffi", "version": "15.2.1"}, "implementation": "Pact-Rust", "version": "1.2.4"}})
    };
    let r = s
        .client
        .post(href)
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::CREATED);
}

struct Env {
    test: String,
    production: String,
}

/// The capture's seed: pacts, provider v1 verifying everything on main, two environments.
async fn seed(s: &common::TestServer) -> Env {
    publish(
        s,
        "Consumer",
        "1.0.0",
        "main",
        vec!["dev"],
        &pact("Consumer", |_| {}),
    )
    .await;
    publish(s, "Web", "1", "main", vec![], &pact("Web", |_| {})).await;
    verify(s, "1", "main", "Consumer/version/1.0.0", true).await;
    verify(s, "1", "main", "Web/version/1", true).await;
    let test: Value = post(
        s,
        "/environments",
        &json!({"name": "test", "displayName": "Test", "production": false}),
    )
    .await
    .json()
    .await
    .unwrap();
    let production: Value = post(s, "/environments", &json!({"name": "production", "displayName": "Production", "production": true, "contacts": [{"name": "Ops", "details": {"emailAddress": "ops@example.com"}}]})).await.json().await.unwrap();
    Env {
        test: test["uuid"].as_str().unwrap().to_owned(),
        production: production["uuid"].as_str().unwrap().to_owned(),
    }
}

async fn record_deployment(
    s: &common::TestServer,
    pacticipant: &str,
    version: &str,
    env_uuid: &str,
    body: Value,
) -> reqwest::Response {
    post(s, &format!("/pacticipants/{pacticipant}/versions/{version}/deployed-versions/environment/{env_uuid}"), &body).await
}

#[tokio::test]
async fn environment_documents_match_the_ruby_broker() {
    let s = common::start().await;
    let r = post(
        &s,
        "/environments",
        &json!({"name": "test", "displayName": "Test", "production": false}),
    )
    .await;
    assert_eq!(r.status(), StatusCode::CREATED);
    assert_eq!(r.headers()["content-type"], HAL);
    let test: Value = r.json().await.unwrap();
    let uuid = test["uuid"].as_str().unwrap().to_owned();
    post(&s, "/environments", &json!({"name": "production", "displayName": "Production", "production": true, "contacts": [{"name": "Ops", "details": {"emailAddress": "ops@example.com"}}]})).await;
    let r = post(
        &s,
        "/environments",
        &json!({"name": "test", "production": false}),
    )
    .await;
    assert_eq!(r.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        r.json::<Value>().await.unwrap(),
        json!({"errors": {"name": ["name 'test' is already used by an existing environment."]}})
    );
    let r = s.client.put(format!("{}/environments/{uuid}", s.base_url)).header("content-type", "application/json").json(&json!({"name": "test", "displayName": "Test env", "production": false, "contacts": [{"name": "QA"}]})).send().await.unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    assert!(r.json::<Value>().await.unwrap().get("updatedAt").is_some());
    let r = get(&s, "/environments").await;
    assert_matches(&s, r, "GET_environments.json", StatusCode::OK).await;
    let r = get(&s, &format!("/environments/{uuid}")).await;
    assert_matches(&s, r, "GET_environment_test.json", StatusCode::OK).await;
    for (body, name) in [
        (json!({}), "POST_environments_err_empty.json"),
        (
            json!({"name": "bad name!", "production": "yes"}),
            "POST_environments_err_types.json",
        ),
        (
            json!({"name": "ok", "displayName": ""}),
            "POST_environments_err_no_production.json",
        ),
    ] {
        let r = post(&s, "/environments", &body).await;
        assert_matches(&s, r, name, StatusCode::BAD_REQUEST).await;
    }
    for path in [
        "/environments/00000000-0000-0000-0000-000000000000",
        "/deployed-versions/00000000-0000-0000-0000-000000000000",
    ] {
        let r = get(&s, path).await;
        assert_eq!(r.status(), StatusCode::NOT_FOUND, "{path}");
        assert_eq!(r.headers()["content-type"], JSON, "{path}");
    }
    let r = s
        .client
        .put(format!(
            "{}/environments/00000000-0000-0000-0000-000000000000",
            s.base_url
        ))
        .header("content-type", "application/json")
        .json(&json!({"name": "x", "production": false}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::NOT_FOUND);
    for (path, allowed) in [
        ("/environments", "GET, POST, OPTIONS"),
        (
            &format!("/environments/{uuid}"),
            "GET, PUT, DELETE, OPTIONS",
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
    let r = s
        .client
        .delete(format!("{}/environments/{uuid}", s.base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        get(&s, &format!("/environments/{uuid}")).await.status(),
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn deployment_documents_match_the_ruby_broker() {
    let s = common::start().await;
    let env = seed(&s).await;
    let r = s.client.put(format!("{}/environments/{}", s.base_url, env.test)).header("content-type", "application/json").json(&json!({"name": "test", "displayName": "Test env", "production": false, "contacts": [{"name": "QA"}]})).send().await.unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    let r = record_deployment(&s, "Provider", "1", &env.test, json!({})).await;
    assert_eq!(r.status(), StatusCode::CREATED);
    let first: Value = r.json().await.unwrap();
    assert_eq!(first["currentlyDeployed"], true);
    assert_eq!(first["_embedded"]["environment"]["displayName"], "Test env");
    let r = record_deployment(&s, "Consumer", "1.0.0", &env.test, json!({})).await;
    let consumer_dv: Value = r.json().await.unwrap();
    let r = record_deployment(&s, "Consumer", "1.0.0", &env.test, json!({})).await;
    assert_eq!(
        r.status(),
        StatusCode::CREATED,
        "re-recording answers 201 with the same record"
    );
    assert_eq!(
        r.json::<Value>().await.unwrap()["uuid"],
        consumer_dv["uuid"]
    );
    let r = record_deployment(
        &s,
        "Web",
        "1",
        &env.test,
        json!({"applicationInstance": "blue", "target": "blue"}),
    )
    .await;
    let web: Value = r.json().await.unwrap();
    assert_eq!(web["applicationInstance"], "blue");
    assert_eq!(web["target"], "blue");

    let r = get(&s, "/pacticipants/Consumer/versions/1.0.0").await;
    assert_matches(
        &s,
        r,
        "GET_pacticipants_Consumer_versions_1.0.0.json",
        StatusCode::OK,
    )
    .await;
    let r = get(
        &s,
        &format!(
            "/environments/{}/deployed-versions/currently-deployed",
            env.test
        ),
    )
    .await;
    assert_matches(&s, r, "GET_currently-deployed.json", StatusCode::OK).await;
    let r = get(
        &s,
        &format!(
            "/environments/{}/deployed-versions/currently-deployed?pacticipant=Consumer",
            env.test
        ),
    )
    .await;
    assert_matches(
        &s,
        r,
        "GET_currently-deployed_consumer.json",
        StatusCode::OK,
    )
    .await;
    let r = get(
        &s,
        &format!(
            "/environments/{}/released-versions/currently-supported",
            env.test
        ),
    )
    .await;
    assert_matches(&s, r, "GET_currently-supported_empty.json", StatusCode::OK).await;
    let dv = consumer_dv["uuid"].as_str().unwrap().to_owned();
    let r = get(&s, &format!("/deployed-versions/{dv}")).await;
    assert_matches(&s, r, "GET_deployed-version.json", StatusCode::OK).await;
    let r = record_deployment(&s, "Consumer", "9.9.9", &env.test, json!({})).await;
    assert_eq!(r.status(), StatusCode::NOT_FOUND);
    assert_eq!(r.headers()["content-type"], JSON);
    let r = record_deployment(
        &s,
        "Consumer",
        "1.0.0",
        "00000000-0000-0000-0000-000000000000",
        json!({}),
    )
    .await;
    assert_eq!(r.status(), StatusCode::NOT_FOUND);
    for (path, allowed) in [
        (format!("/deployed-versions/{dv}"), "GET, PATCH, OPTIONS"),
        (
            format!(
                "/environments/{}/deployed-versions/currently-deployed",
                env.test
            ),
            "GET, OPTIONS",
        ),
        (
            format!(
                "/pacticipants/Consumer/versions/1.0.0/deployed-versions/environment/{}",
                env.test
            ),
            "GET, POST, OPTIONS",
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

    // The capture published Consumer 2.0.0 on main (the can-i-deploy section) before
    // recording releases, so 1.0.0 is no longer the branch head from here on.
    publish(
        &s,
        "Consumer",
        "2.0.0",
        "main",
        vec![],
        &pact("Consumer", |p| {
            p["interactions"][0]["response"]["body"]["status"] = json!("closed")
        }),
    )
    .await;

    // Releases and support ended.
    let r = post(
        &s,
        &format!(
            "/pacticipants/Consumer/versions/1.0.0/released-versions/environment/{}",
            env.production
        ),
        &json!({}),
    )
    .await;
    assert_eq!(r.status(), StatusCode::CREATED);
    let rv: Value = r.json().await.unwrap();
    let r = post(
        &s,
        &format!(
            "/pacticipants/Consumer/versions/1.0.0/released-versions/environment/{}",
            env.production
        ),
        &json!({}),
    )
    .await;
    assert_eq!(r.status(), StatusCode::OK, "already released");
    let r = get(
        &s,
        &format!(
            "/environments/{}/released-versions/currently-supported",
            env.production
        ),
    )
    .await;
    assert_matches(&s, r, "GET_currently-supported.json", StatusCode::OK).await;
    let r = get(&s, &format!("/environments/{}/released-versions/currently-supported?pacticipant=Consumer&version=1.0.0", env.production)).await;
    assert_matches(
        &s,
        r,
        "GET_currently-supported_filtered.json",
        StatusCode::OK,
    )
    .await;
    let rvu = rv["uuid"].as_str().unwrap().to_owned();
    let r = get(&s, &format!("/released-versions/{rvu}")).await;
    assert_matches(&s, r, "GET_released-version.json", StatusCode::OK).await;
    let r = post(
        &s,
        "/pacts/provider/Provider/for-verification",
        &json!({"consumerVersionSelectors": [{"released": true}]}),
    )
    .await;
    assert_matches(&s, r, "POST_for-verification_released.json", StatusCode::OK).await;
    let r = get(&s, "/pacticipants/Consumer/versions/1.0.0").await;
    assert_matches(
        &s,
        r,
        "GET_pacticipants_Consumer_versions_1.0.0_released.json",
        StatusCode::OK,
    )
    .await;
    let r = s
        .client
        .patch(format!("{}/released-versions/{rvu}", s.base_url))
        .header("content-type", "application/merge-patch+json")
        .body(r#"{"currentlySupported":false}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    let ended: Value = r.json().await.unwrap();
    assert_eq!(ended["currentlySupported"], false);
    assert!(ended.get("supportEndedAt").is_some());

    // Undeployment and a newer deployment.
    let r = s
        .client
        .patch(format!("{}/deployed-versions/{dv}", s.base_url))
        .header("content-type", "application/merge-patch+json")
        .body(r#"{"currentlyDeployed":false}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    let undeployed: Value = r.json().await.unwrap();
    assert_eq!(undeployed["currentlyDeployed"], false);
    assert!(undeployed.get("undeployedAt").is_some());
    publish(
        &s,
        "Consumer",
        "2.0.0",
        "main",
        vec![],
        &pact("Consumer", |p| {
            p["interactions"][0]["response"]["body"]["status"] = json!("closed")
        }),
    )
    .await;
    let web_dv = web["uuid"].as_str().unwrap().to_owned();
    s.client
        .patch(format!("{}/deployed-versions/{web_dv}", s.base_url))
        .header("content-type", "application/merge-patch+json")
        .body(r#"{"currentlyDeployed":false}"#)
        .send()
        .await
        .unwrap();
    record_deployment(&s, "Consumer", "2.0.0", &env.test, json!({})).await;
    let r = get(&s, "/pacticipants/Consumer/versions/1.0.0").await;
    assert_matches(
        &s,
        r,
        "GET_pacticipants_Consumer_versions_1.0.0_after.json",
        StatusCode::OK,
    )
    .await;
    let r = get(&s, "/pacticipants/Consumer/versions/2.0.0").await;
    assert_matches(
        &s,
        r,
        "GET_pacticipants_Consumer_versions_2.0.0_after.json",
        StatusCode::OK,
    )
    .await;
    let r = get(
        &s,
        &format!(
            "/environments/{}/deployed-versions/currently-deployed",
            env.test
        ),
    )
    .await;
    assert_matches(&s, r, "GET_currently-deployed_after.json", StatusCode::OK).await;
    let r = get(&s, &format!("/deployed-versions/{dv}")).await;
    assert_matches(
        &s,
        r,
        "GET_deployed-version_undeployed.json",
        StatusCode::OK,
    )
    .await;
    let r = get(&s, &format!("/released-versions/{rvu}")).await;
    assert_matches(
        &s,
        r,
        "GET_released-version_support_ended.json",
        StatusCode::OK,
    )
    .await;
    let r = s
        .client
        .delete(format!("{}/environments/{}", s.base_url, env.production))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::NO_CONTENT);
    let r = get(&s, "/environments").await;
    assert_matches(&s, r, "GET_environments_after_delete.json", StatusCode::OK).await;
}

/// The capture's state when the can-i-deploy fixtures were taken.
async fn seed_for_matrix(s: &common::TestServer) -> Env {
    let env = seed(s).await;
    s.client.put(format!("{}/environments/{}", s.base_url, env.test)).header("content-type", "application/json").json(&json!({"name": "test", "displayName": "Test env", "production": false, "contacts": [{"name": "QA"}]})).send().await.unwrap();
    record_deployment(s, "Provider", "1", &env.test, json!({})).await;
    record_deployment(s, "Consumer", "1.0.0", &env.test, json!({})).await;
    record_deployment(
        s,
        "Web",
        "1",
        &env.test,
        json!({"applicationInstance": "blue"}),
    )
    .await;
    env
}

const Q_CONSUMER_1: &str =
    "q%5B%5D%5Bpacticipant%5D=Consumer&q%5B%5D%5Bversion%5D=1.0.0&latestby=cvp&environment=test";

#[tokio::test]
async fn matrix_documents_match_the_ruby_broker() {
    let s = common::start().await;
    seed_for_matrix(&s).await;
    let r = get(&s, &format!("/matrix?{Q_CONSUMER_1}")).await;
    assert_eq!(r.headers()["content-type"], HAL);
    assert_matches(&s, r, "GET_matrix_consumer_1.0.0_test.json", StatusCode::OK).await;

    publish(
        &s,
        "Consumer",
        "2.0.0",
        "main",
        vec![],
        &pact("Consumer", |p| {
            p["interactions"][0]["response"]["body"]["status"] = json!("closed")
        }),
    )
    .await;
    let r = get(&s, "/matrix?q%5B%5D%5Bpacticipant%5D=Consumer&q%5B%5D%5Bversion%5D=2.0.0&latestby=cvp&environment=test").await;
    assert_matches(
        &s,
        r,
        "GET_matrix_consumer_2.0.0_test_unverified.json",
        StatusCode::OK,
    )
    .await;
    // Provider 2 on main fails everything it verifies (the capture's second verifier run);
    // then provider 1 verifies the latest pacts from branch main: Consumer 2.0.0 fails
    // (its answers fit 1.0.0 only) and Web 1 passes again.
    verify(&s, "2", "main", "Consumer/version/2.0.0", false).await;
    verify(&s, "2", "main", "Web/version/1", false).await;
    verify(&s, "1", "main", "Consumer/version/2.0.0", false).await;
    verify(&s, "1", "main", "Web/version/1", true).await;
    let r = get(&s, "/matrix?q%5B%5D%5Bpacticipant%5D=Consumer&q%5B%5D%5Bversion%5D=2.0.0&latestby=cvp&environment=test").await;
    assert_matches(
        &s,
        r,
        "GET_matrix_consumer_2.0.0_test_failed.json",
        StatusCode::OK,
    )
    .await;
    let r = get(&s, "/matrix?q%5B%5D%5Bpacticipant%5D=Provider&q%5B%5D%5Bversion%5D=1&latestby=cvp&environment=test").await;
    assert_matches(&s, r, "GET_matrix_provider_1_test.json", StatusCode::OK).await;
    // Provider 2 verified Consumer 2.0.0 (not deployed) and Web 1; the capture's v2 run
    // used the default selectors, which also covered Consumer 1.0.0 via... no: main was
    // 2.0.0 by then, so Consumer 1.0.0 has no v2 result in the capture either.
    verify(&s, "2", "main", "Consumer/version/1.0.0", false).await;
    let r = get(&s, "/matrix?q%5B%5D%5Bpacticipant%5D=Provider&q%5B%5D%5Bversion%5D=2&latestby=cvp&environment=test").await;
    assert_matches(&s, r, "GET_matrix_provider_2_test.json", StatusCode::OK).await;
    let r = get(&s, "/matrix?q%5B%5D%5Bpacticipant%5D=Consumer&q%5B%5D%5Bversion%5D=9.9.9&latestby=cvp&environment=test").await;
    assert_matches(&s, r, "GET_matrix_consumer_9.9.9_test.json", StatusCode::OK).await;
    for (query, name) in [
        (
            "q%5B%5D%5Bpacticipant%5D=Consumer&q%5B%5D%5Bversion%5D=1.0.0&latestby=cvp&environment=nope",
            "GET_matrix_unknown_environment.json",
        ),
        (
            "q%5B%5D%5Bpacticipant%5D=Nope&q%5B%5D%5Bversion%5D=1&latestby=cvp&environment=test",
            "GET_matrix_unknown_pacticipant.json",
        ),
    ] {
        let r = get(&s, &format!("/matrix?{query}")).await;
        assert_matches(&s, r, name, StatusCode::BAD_REQUEST).await;
    }
    let r = get(&s, "/matrix?q%5B%5D%5Bpacticipant%5D=Consumer&q%5B%5D%5Bversion%5D=1.0.0&latestby=cvp&latest=true").await;
    assert_matches(
        &s,
        r,
        "GET_matrix_consumer_1.0.0_latest.json",
        StatusCode::OK,
    )
    .await;
    let r = get(&s, "/matrix?q%5B%5D%5Bpacticipant%5D=Consumer&q%5B%5D%5Blatest%5D=true&q%5B%5D%5BmainBranch%5D=true&latestby=cvp&environment=test").await;
    assert_matches(
        &s,
        r,
        "GET_matrix_consumer_main_branch_test.json",
        StatusCode::OK,
    )
    .await;
    let r = get(&s, "/matrix?q%5B%5D%5Bpacticipant%5D=Consumer&q%5B%5D%5Blatest%5D=true&q%5B%5D%5Btag%5D=dev&latestby=cvp&environment=test").await;
    assert_matches(
        &s,
        r,
        "GET_matrix_consumer_latest_dev_test.json",
        StatusCode::OK,
    )
    .await;
    let r = get(&s, "/matrix?q%5B%5D%5Bpacticipant%5D=Consumer&q%5B%5D%5Blatest%5D=true&q%5B%5D%5Bbranch%5D=main&latestby=cvp&environment=test").await;
    assert_matches(
        &s,
        r,
        "GET_matrix_consumer_branch_main_test.json",
        StatusCode::OK,
    )
    .await;
    let r = get(&s, "/matrix?q%5B%5D%5Bpacticipant%5D=Consumer&q%5B%5D%5Bversion%5D=1.0.0&latestby=cvp&latest=true&tag=prod").await;
    assert_matches(
        &s,
        r,
        "GET_matrix_consumer_1.0.0_to_prod_missing.json",
        StatusCode::OK,
    )
    .await;
    s.client
        .put(format!(
            "{}/pacticipants/Provider/versions/1/tags/prod",
            s.base_url
        ))
        .header("content-type", "application/json")
        .send()
        .await
        .unwrap();
    let r = get(&s, "/matrix?q%5B%5D%5Bpacticipant%5D=Consumer&q%5B%5D%5Bversion%5D=1.0.0&latestby=cvp&latest=true&tag=prod").await;
    assert_matches(
        &s,
        r,
        "GET_matrix_consumer_1.0.0_to_prod.json",
        StatusCode::OK,
    )
    .await;
    let r = get(&s, "/matrix?q%5B%5D%5Bpacticipant%5D=Consumer&q%5B%5D%5Bversion%5D=1.0.0&q%5B%5D%5Bpacticipant%5D=Provider&q%5B%5D%5Bversion%5D=1&latestby=cvpv").await;
    assert_matches(
        &s,
        r,
        "GET_matrix_consumer_1.0.0_provider_1.json",
        StatusCode::OK,
    )
    .await;
    let r = get(&s, "/matrix?q%5B%5D%5Bpacticipant%5D=Consumer&q%5B%5D%5Bversion%5D=1.0.0&q%5B%5D%5Bpacticipant%5D=Provider&q%5B%5D%5Bversion%5D=2&latestby=cvpv").await;
    assert_matches(
        &s,
        r,
        "GET_matrix_consumer_1.0.0_provider_2.json",
        StatusCode::OK,
    )
    .await;
    let r = get(&s, "/matrix?q%5B%5D%5Bpacticipant%5D=Consumer&q%5B%5D%5Bversion%5D=2.0.0&latestby=cvp&environment=test&ignore%5B%5D%5Bpacticipant%5D=Provider").await;
    assert_matches(
        &s,
        r,
        "GET_matrix_consumer_2.0.0_test_ignore_provider.json",
        StatusCode::OK,
    )
    .await;
    let r = get(&s, "/matrix?q%5B%5D%5Bpacticipant%5D=Consumer&q%5B%5D%5Bversion%5D=1.0.0&latestby=cvp&environment=production").await;
    assert_matches(
        &s,
        r,
        "GET_matrix_consumer_1.0.0_production_empty.json",
        StatusCode::OK,
    )
    .await;
    let r = get(
        &s,
        "/matrix?q%5B%5D%5Bpacticipant%5D=Consumer&q%5B%5D%5Bversion%5D=1.0.0",
    )
    .await;
    assert_matches(
        &s,
        r,
        "GET_matrix_consumer_1.0.0_no_latestby.json",
        StatusCode::OK,
    )
    .await;
    let r = get(&s, "/matrix?q%5B%5D%5Bpacticipant%5D=Consumer").await;
    assert_matches(&s, r, "GET_matrix_consumer_only.json", StatusCode::OK).await;
    let r = get(&s, "/matrix/provider/Provider/consumer/Consumer").await;
    assert_matches(&s, r, "GET_matrix_integration.json", StatusCode::OK).await;
    let r = get(
        &s,
        "/can-i-deploy?pacticipant=Consumer&version=1.0.0&environment=test",
    )
    .await;
    assert_matches(&s, r, "GET_can-i-deploy_environment.json", StatusCode::OK).await;
    let r = get(
        &s,
        "/can-i-deploy?pacticipant=Consumer&version=1.0.0&to=prod",
    )
    .await;
    assert_matches(&s, r, "GET_can-i-deploy_to_tag.json", StatusCode::OK).await;
    let r = get(&s, "/matrix").await;
    assert_eq!(r.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        r.json::<Value>().await.unwrap(),
        json!({"errors": ["Please provide 1 or more version selectors."]})
    );

    // Deployment-based selectors for verification.
    let r = post(
        &s,
        "/pacts/provider/Provider/for-verification",
        &json!({"consumerVersionSelectors": [{"deployed": true}]}),
    )
    .await;
    assert_matches(&s, r, "POST_for-verification_deployed.json", StatusCode::OK).await;
    let r = post(
        &s,
        "/pacts/provider/Provider/for-verification",
        &json!({"consumerVersionSelectors": [{"deployedOrReleased": true}]}),
    )
    .await;
    assert_matches(
        &s,
        r,
        "POST_for-verification_deployedOrReleased.json",
        StatusCode::OK,
    )
    .await;
    let r = get(&s, "/pacts/provider/Provider/for-verification").await;
    assert_matches(
        &s,
        r,
        "GET_for-verification_with_deployments.json",
        StatusCode::OK,
    )
    .await;
}
