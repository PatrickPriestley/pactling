//! Retrieval lists: the capture's mutation sequence is replayed against pactling, then
//! every list fixture captured after it is compared (hosts rewritten, timestamps stripped).
mod common;
use base64::Engine;
use common::{fixture, stable};
use reqwest::StatusCode;
use serde_json::{Value, json};

const HAL: &str = "application/hal+json;charset=utf-8";

fn pact_named(consumer: &str, provider: &str, changed: bool) -> Value {
    let mut p: Value =
        serde_json::from_str(include_str!("fixtures/pact/consumer-provider.json")).unwrap();
    p["consumer"]["name"] = json!(consumer);
    p["provider"]["name"] = json!(provider);
    if changed {
        p["interactions"][0]["response"]["body"]["status"] = json!("closed");
    }
    p
}

fn contract(consumer: &str, provider: &str, content: &Value) -> Value {
    json!({
        "consumerName": consumer, "providerName": provider, "specification": "pact", "contentType": "application/json",
        "content": base64::engine::general_purpose::STANDARD.encode(serde_json::to_vec(content).unwrap())
    })
}

async fn post(s: &common::TestServer, body: Value) {
    let r = s
        .client
        .post(format!("{}/contracts/publish", s.base_url))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert!(
        r.status().is_success(),
        "{}: {}",
        r.status(),
        r.text().await.unwrap()
    );
}

async fn put(s: &common::TestServer, path: &str, body: &str) -> StatusCode {
    s.client
        .put(format!("{}{path}", s.base_url))
        .header("content-type", "application/json")
        .body(body.to_owned())
        .send()
        .await
        .unwrap()
        .status()
}

/// Everything scripts/capture-ruby-fixtures.sh does before it captures the retrieval
/// documents, in the same order.
async fn replay_capture_sequence(s: &common::TestServer) {
    let c1 = pact_named("Consumer", "Provider", false);
    let c2 = pact_named("Consumer", "Provider", true);
    let pub_ = |v: &str, branch: Option<&str>, tags: Vec<&str>, c: &Value| {
        let mut b = json!({ "pacticipantName": "Consumer", "pacticipantVersionNumber": v, "tags": tags, "contracts": [contract("Consumer", "Provider", c)] });
        if let Some(br) = branch {
            b["branch"] = json!(br);
        }
        b
    };
    post(s, pub_("1.0.0", Some("main"), vec!["dev"], &c1)).await;
    post(s, pub_("1.0.0", Some("main"), vec!["dev"], &c1)).await;
    post(s, pub_("1.0.0", Some("main"), vec!["dev"], &c1)).await;
    post(s, pub_("1.0.1", None, vec![], &c1)).await;
    post(s, pub_("1.0.2", None, vec!["prod", "test"], &c1)).await;
    post(s, pub_("2.0.0", Some("main"), vec![], &c2)).await;
    post(s, pub_("2.0.1", Some("main"), vec![], &c2)).await;
    post(s, pub_("3.0.0", Some("feat"), vec![], &c1)).await;
    let mut b = pub_("3.0.1", Some("feat"), vec![], &c1);
    b.as_object_mut().unwrap().remove("tags");
    b["buildUrl"] = json!("https://ci.example.com/1");
    post(s, b).await;
    post(s, json!({ "pacticipantName": "Web", "pacticipantVersionNumber": "1", "branch": "main", "tags": ["dev"],
        "contracts": [contract("Web", "Provider", &pact_named("Web", "Provider", false)), contract("Web", "Other", &pact_named("Web", "Other", false))] })).await;
    let p1 = serde_json::to_string(&c1).unwrap();
    assert_eq!(
        put(
            s,
            "/pacts/provider/Provider/consumer/Consumer/version/4.0.0",
            &p1
        )
        .await,
        StatusCode::CREATED
    );
    put(
        s,
        "/pacts/provider/Provider/consumer/Consumer/version/4.0.0",
        &p1,
    )
    .await;
    let minimal = r#"{"consumer":{"name":"Consumer"},"provider":{"name":"Provider"},"interactions":[],"metadata":{}}"#;
    put(
        s,
        "/pacts/provider/Provider/consumer/Consumer/version/4.0.2",
        minimal,
    )
    .await;
    put(
        s,
        "/pacts/provider/Provider/consumer/Consumer/version/4.0.4",
        r#"{"interactions":[]}"#,
    )
    .await;
    put(
        s,
        "/pacts/provider/Provider/consumer/Consumer/version/1.0.0%2Bbuild",
        minimal,
    )
    .await;
    let three = r#"{"consumer":{"name":"Consumer"},"provider":{"name":"Provider"},"interactions":[{"description":"zeta","request":{"method":"GET","path":"/a"},"response":{"status":200}},{"description":"alpha","providerState":"zz","request":{"method":"GET","path":"/b"},"response":{"status":200}},{"description":"mid","request":{"method":"POST","path":"/c"},"response":{"status":201}}],"metadata":{"pactSpecification":{"version":"2.0.0"}}}"#;
    put(
        s,
        "/pacts/provider/Provider/consumer/Consumer/version/7.0.0",
        three,
    )
    .await;
    put(s, "/pacticipants/Consumer/versions/8.0.0", "{}").await;
    put(
        s,
        "/pacticipants/Consumer/versions/8.0.0",
        r#"{"buildUrl":"https://ci/8"}"#,
    )
    .await;
    put(s, "/pacticipants/Brand_New/versions/1", "{}").await;
    put(s, "/pacticipants/Consumer/versions/9.0.0/tags/t1", "").await;
    put(s, "/pacticipants/Consumer/versions/9.0.0/tags/t1", "").await;
    put(s, "/pacticipants/Consumer/versions/9.0.0/tags/t2", "").await;
    put(
        s,
        "/pacticipants/Consumer/branches/release/versions/10.0.0",
        "",
    )
    .await;
    put(
        s,
        "/pacticipants/Consumer/branches/release/versions/10.0.0",
        "",
    )
    .await;
    put(
        s,
        "/pacticipants/Consumer/branches/release/versions/9.0.0",
        "",
    )
    .await;
}

/// The verification-results href of a "latest" pact carries base64 metadata holding the
/// broker's internal consumer version id, which legitimately differs between brokers.
fn without_metadata(v: Value) -> Value {
    let text = serde_json::to_string(&v).unwrap();
    let re = regex::Regex::new(r"/metadata/[A-Za-z0-9_=-]+").unwrap();
    serde_json::from_str(&re.replace_all(&text, "")).unwrap()
}

fn sort_branches(doc: &mut Value) {
    let by_name = |b: &Value| b["name"].as_str().unwrap().to_owned();
    doc["_embedded"]["branches"]
        .as_array_mut()
        .unwrap()
        .sort_by_key(by_name);
    doc["_links"]["pb:branches"]
        .as_array_mut()
        .unwrap()
        .sort_by_key(by_name);
}

#[tokio::test]
async fn every_retrieval_document_matches_the_ruby_broker() {
    let s = common::start().await;
    replay_capture_sequence(&s).await;
    let fixtures = [
        "GET_pacts_latest",
        "GET_pacts_provider_Provider",
        "GET_pacts_provider_Provider_latest",
        "GET_pacts_provider_Provider_latest_dev",
        "GET_pacts_provider_Provider_tag_dev",
        "GET_pacts_provider_Provider_branch",
        "GET_pacts_provider_Provider_branch_main",
        "GET_pacts_provider_Provider_branch_latest",
        "GET_pacts_provider_Provider_branch_main_latest",
        "GET_pacts_provider_Provider_consumer_Consumer_versions",
        "GET_pacts_provider_Provider_consumer_Consumer_tag_dev",
        "GET_pacts_provider_Provider_consumer_Consumer_branch_main",
        "GET_pacts_provider_Provider_consumer_Consumer_branch_main_latest_versions",
        "GET_pacts_provider_Provider_consumer_Consumer_branch_latest_versions",
        "GET_pacts_provider_Provider_consumer_Consumer_branch_versions",
        "GET_pacts_provider_Provider_consumer_Consumer_latest",
        "GET_pacts_provider_Provider_consumer_Consumer_latest_dev",
        "GET_pacts_provider_Provider_consumer_Consumer_latest-untagged",
        "GET_pacts_provider_Provider_consumer_Consumer_pact-version_0350e2d94a510324ee31cd7375bf1660e5d8c485",
        "GET_integrations",
        "GET_pacticipants",
        "GET_pacticipants_Consumer",
        "GET_pacticipants_Consumer_versions",
        "GET_pacticipants_Consumer_branches",
        "GET_pacticipants_Consumer_latest-version",
        "GET_pacticipants_Consumer_latest-version_dev",
        "GET_pacticipants_Consumer_branches_main_latest-version",
        "GET_pacticipants_Web",
        "GET_pacticipants_Provider",
    ];
    for name in fixtures {
        let path = name.trim_start_matches("GET_").replace('_', "/");
        let path = if path == "index" {
            String::new()
        } else {
            format!("/{path}")
        };
        let r = s
            .client
            .get(format!("{}{path}", s.base_url))
            .header("accept", "application/hal+json")
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::OK, "{path}");
        assert_eq!(r.headers()["content-type"], HAL, "{path}");
        let got: Value = r.json().await.unwrap();
        let want = stable(fixture(&format!("phase2/{name}.json"), &s.base_url));
        let (mut got, mut want) = (without_metadata(stable(got)), without_metadata(want));
        if name == "GET_pacticipants_Consumer_branches" {
            // Ordered by last update, which the replay cannot reproduce to the second.
            sort_branches(&mut got);
            sort_branches(&mut want);
        }
        if got != want
            && let Ok(dir) = std::env::var("PACTLING_DUMP_DIR")
        {
            std::fs::write(
                format!("{dir}/{name}.got.json"),
                serde_json::to_string_pretty(&got).unwrap(),
            )
            .unwrap();
            std::fs::write(
                format!("{dir}/{name}.want.json"),
                serde_json::to_string_pretty(&want).unwrap(),
            )
            .unwrap();
        }
        assert_eq!(got, want, "{path} differs from {name}");
    }
}

#[tokio::test]
async fn list_routes_for_an_unknown_provider_or_integration() {
    let s = common::start().await;
    let r = s
        .client
        .get(format!("{}/pacts/provider/Nope/latest", s.base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::NOT_FOUND);
    assert_eq!(r.headers()["content-type"], HAL);
    assert_eq!(
        r.json::<Value>().await.unwrap(),
        json!({"error": "No provider with name 'Nope' found"})
    );
    for path in [
        "/pacts/provider/Nope/consumer/Consumer/latest",
        "/pacts/provider/Provider/consumer/Nope/latest",
        "/pacts/provider/Provider/consumer/Nope/versions",
    ] {
        let r = s
            .client
            .get(format!("{}{path}", s.base_url))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::NOT_FOUND, "{path}");
        assert_eq!(
            r.headers()["content-type"],
            "application/json;charset=utf-8",
            "{path}"
        );
    }
}

#[tokio::test]
async fn list_routes_allow_get_and_options_only() {
    let s = common::start().await;
    put(
        &s,
        "/pacts/provider/Provider/consumer/Consumer/version/1",
        r#"{"interactions":[]}"#,
    )
    .await;
    for (path, allowed) in [
        ("/pacts/latest", "GET, OPTIONS"),
        ("/pacts/provider/Provider/latest", "GET, OPTIONS"),
        (
            "/pacts/provider/Provider/consumer/Consumer/versions",
            "GET, DELETE, OPTIONS",
        ),
        (
            "/pacts/provider/Provider/consumer/Consumer/latest",
            "GET, OPTIONS",
        ),
        (
            "/pacts/provider/Provider/consumer/Consumer/pact-version/ed7c1aebc3e11064ce9afc29297e1e36203583cd",
            "GET, OPTIONS",
        ),
    ] {
        let r = s
            .client
            .request(reqwest::Method::OPTIONS, format!("{}{path}", s.base_url))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::OK, "{path}");
        assert_eq!(
            r.headers()["access-control-allow-methods"],
            allowed,
            "{path}"
        );
        let r = s
            .client
            .head(format!("{}{path}", s.base_url))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::METHOD_NOT_ALLOWED, "{path}");
        assert_eq!(r.headers()["allow"], allowed, "{path}");
    }
}

#[tokio::test]
async fn bulk_delete_of_an_integrations_pacts_matches_the_ruby_broker() {
    // Probed on Ruby 2.121.2: 204, idempotent, versions and pacticipants survive, and
    // unknown names are the HAL 404 naming the role.
    let s = common::start().await;
    let pact = serde_json::to_string(&pact_named("Consumer", "BulkP", false)).unwrap();
    put(
        &s,
        "/pacts/provider/BulkP/consumer/Consumer/version/b1",
        &pact,
    )
    .await;
    put(
        &s,
        "/pacts/provider/BulkP/consumer/Consumer/version/b2",
        &pact,
    )
    .await;
    let del = |path: &'static str| s.client.delete(format!("{}{path}", s.base_url)).send();
    assert_eq!(
        del("/pacts/provider/BulkP/consumer/Consumer/versions")
            .await
            .unwrap()
            .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        s.client
            .get(format!(
                "{}/pacts/provider/BulkP/consumer/Consumer/latest",
                s.base_url
            ))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        s.client
            .get(format!("{}/pacticipants/Consumer/versions/b1", s.base_url))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        s.client
            .get(format!("{}/pacticipants/BulkP", s.base_url))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        del("/pacts/provider/BulkP/consumer/Consumer/versions")
            .await
            .unwrap()
            .status(),
        StatusCode::NO_CONTENT,
        "idempotent"
    );
    for (path, msg) in [
        (
            "/pacts/provider/Nope/consumer/Consumer/versions",
            "No provider with name 'Nope' found",
        ),
        (
            "/pacts/provider/BulkP/consumer/Nope/versions",
            "No consumer with name 'Nope' found",
        ),
    ] {
        let r = s
            .client
            .delete(format!("{}{path}", s.base_url))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::NOT_FOUND, "{path}");
        assert_eq!(r.headers()["content-type"], HAL, "{path}");
        assert_eq!(
            r.json::<Value>().await.unwrap(),
            json!({"error": msg}),
            "{path}"
        );
    }
}
