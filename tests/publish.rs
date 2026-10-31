//! Publishing: `POST /contracts/publish` and the legacy `PUT /pacts/...`, compared with the
//! phase 2 fixtures captured from Ruby broker 2.121.2 in the same order they were made.
mod common;
use base64::Engine;
use common::{fixture, stable};
use reqwest::StatusCode;
use serde_json::{Value, json};

const HAL: &str = "application/hal+json;charset=utf-8";

fn pact() -> Value {
    serde_json::from_str(include_str!("fixtures/pact/consumer-provider.json")).unwrap()
}

fn changed_pact() -> Value {
    let mut p = pact();
    p["interactions"][0]["response"]["body"]["status"] = json!("closed");
    p
}

fn b64(v: &Value) -> String {
    base64::engine::general_purpose::STANDARD.encode(serde_json::to_vec(v).unwrap())
}

fn contract(consumer: &str, provider: &str, content: &Value) -> Value {
    json!({
        "consumerName": consumer, "providerName": provider, "specification": "pact",
        "contentType": "application/json", "content": b64(content)
    })
}

fn request(
    p: &str,
    v: &str,
    branch: Option<&str>,
    tags: Option<Vec<&str>>,
    contracts: Vec<Value>,
) -> Value {
    let mut body = json!({ "pacticipantName": p, "pacticipantVersionNumber": v });
    if let Some(t) = tags {
        body["tags"] = json!(t);
    }
    if let Some(b) = branch {
        body["branch"] = json!(b);
    }
    body["contracts"] = json!(contracts);
    body
}

async fn publish(s: &common::TestServer, body: &Value) -> reqwest::Response {
    s.client
        .post(format!("{}/contracts/publish", s.base_url))
        .header("content-type", "application/json")
        .header("accept", "application/hal+json")
        .json(body)
        .send()
        .await
        .unwrap()
}

async fn publish_raw(s: &common::TestServer, body: &str) -> reqwest::Response {
    s.client
        .post(format!("{}/contracts/publish", s.base_url))
        .header("content-type", "application/json")
        .body(body.to_owned())
        .send()
        .await
        .unwrap()
}

async fn put_pact(
    s: &common::TestServer,
    provider: &str,
    consumer: &str,
    version: &str,
    body: &str,
) -> reqwest::Response {
    s.client
        .put(format!(
            "{}/pacts/provider/{provider}/consumer/{consumer}/version/{version}",
            s.base_url
        ))
        .header("content-type", "application/json")
        .header("accept", "application/hal+json")
        .body(body.to_owned())
        .send()
        .await
        .unwrap()
}

fn phase2(name: &str, base: &str) -> Value {
    stable(fixture(&format!("phase2/{name}"), base))
}

fn content_type(r: &reqwest::Response) -> String {
    r.headers()
        .get("content-type")
        .map(|v| v.to_str().unwrap().to_owned())
        .unwrap_or_default()
}

fn notices(body: &Value) -> Vec<String> {
    body["notices"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| {
            format!(
                "{}|{}",
                n["type"].as_str().unwrap(),
                n["text"].as_str().unwrap()
            )
        })
        .collect()
}

async fn assert_publish_matches(s: &common::TestServer, body: &Value, fixture_name: &str) -> Value {
    let r = publish(s, body).await;
    let status = r.status();
    let got: Value = r.json().await.unwrap();
    assert_eq!(status, StatusCode::OK, "{fixture_name}: {got}");
    let want = phase2(fixture_name, &s.base_url);
    assert_eq!(stable(got.clone()), want, "{fixture_name}");
    got
}

#[tokio::test]
async fn publish_contracts_sequence_matches_the_ruby_broker() {
    let s = common::start().await;
    let b = &s.base_url;
    let c1 = contract("Consumer", "Provider", &pact());
    let c2 = contract("Consumer", "Provider", &changed_pact());

    // 1. First publish ever, with a branch and a tag (recorded through the proxy; notices
    //    hand-copied from tests/fixtures/cli/phase2/publish-1.0.0.jsonl).
    let r = publish(
        &s,
        &request(
            "Consumer",
            "1.0.0",
            Some("main"),
            Some(vec!["dev"]),
            vec![c1.clone()],
        ),
    )
    .await;
    assert_eq!(r.status(), StatusCode::OK);
    assert_eq!(content_type(&r), HAL);
    let body: Value = r.json().await.unwrap();
    assert_eq!(
        notices(&body),
        [
            "debug|Created Consumer version 1.0.0 with branch main and tags dev".to_owned(),
            "success|Pact successfully published for Consumer version 1.0.0 and provider Provider.".to_owned(),
            format!("debug|  View the published pact at {b}/pacts/provider/Provider/consumer/Consumer/version/1.0.0"),
            "debug|  Events detected: contract_published, contract_content_changed (first time any pact published for this consumer with consumer version tagged dev)".to_owned(),
            "prompt|  Next steps:".to_owned(),
            "prompt|    * Add Pact verification tests to the Provider build. See https://docs.pact.io/go/provider_verification".to_owned(),
            "prompt|    * Configure separate Provider pact verification build and webhook to trigger it when the pact content changes. See https://docs.pact.io/go/webhooks".to_owned(),
        ]
    );
    assert_eq!(
        body["logs"][0],
        json!({"level": "debug", "message": "Created Consumer version 1.0.0 with branch main and tags dev", "deprecationWarning": "Replaced by notices"})
    );
    assert_eq!(body["logs"].as_array().unwrap().len(), 7);
    assert_eq!(
        body["_embedded"]["pacticipant"],
        json!({"name": "Consumer", "_links": {"self": {"href": format!("{b}/pacticipants/Consumer")}}})
    );
    assert_eq!(
        body["_links"]["pb:pacticipant-version-tags"],
        json!([{"title": "Tag", "name": "dev", "href": format!("{b}/pacticipants/Consumer/versions/1.0.0/tags/dev")}])
    );
    assert_eq!(
        body["_links"]["pb:contracts"],
        json!([{"title": "Pact", "name": "Pact between Consumer (1.0.0) and Provider", "href": format!("{b}/pacts/provider/Provider/consumer/Consumer/version/1.0.0")}])
    );

    // 2. The same again: updated, republished with no content changes.
    assert_publish_matches(
        &s,
        &request(
            "Consumer",
            "1.0.0",
            Some("main"),
            Some(vec!["dev"]),
            vec![c1.clone()],
        ),
        "POST_contracts_publish_1.0.0_again.json",
    )
    .await;

    // 3. Different content for the same version: 409. The Ruby broker adds an `info`
    //    notice holding a diff, which pactling does not reproduce.
    let r = publish(
        &s,
        &request(
            "Consumer",
            "1.0.0",
            Some("main"),
            Some(vec![]),
            vec![c2.clone()],
        ),
    )
    .await;
    assert_eq!(r.status(), StatusCode::CONFLICT);
    assert_eq!(content_type(&r), HAL);
    let got: Value = r.json().await.unwrap();
    let want = phase2("POST_contracts_publish_1.0.0_changed.json", b);
    assert_eq!(got["errors"], want["errors"]);
    assert_eq!(
        got["notices"],
        json!(
            want["notices"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|n| n["type"] != "info")
                .collect::<Vec<_>>()
        )
    );
    assert!(got.get("logs").is_none());

    // 4..10. The rest of the captured sequence.
    assert_publish_matches(
        &s,
        &request("Consumer", "1.0.1", None, Some(vec![]), vec![c1.clone()]),
        "POST_contracts_publish_1.0.1_nobranch.json",
    )
    .await;
    assert_publish_matches(
        &s,
        &request(
            "Consumer",
            "1.0.2",
            None,
            Some(vec!["prod", "test"]),
            vec![c1.clone()],
        ),
        "POST_contracts_publish_1.0.2_tags_only.json",
    )
    .await;
    assert_publish_matches(
        &s,
        &request(
            "Consumer",
            "2.0.0",
            Some("main"),
            Some(vec![]),
            vec![c2.clone()],
        ),
        "POST_contracts_publish_2.0.0_main_changed.json",
    )
    .await;
    assert_publish_matches(
        &s,
        &request(
            "Consumer",
            "2.0.1",
            Some("main"),
            Some(vec![]),
            vec![c2.clone()],
        ),
        "POST_contracts_publish_2.0.1_main_same.json",
    )
    .await;
    assert_publish_matches(
        &s,
        &request(
            "Consumer",
            "3.0.0",
            Some("feat"),
            Some(vec![]),
            vec![c1.clone()],
        ),
        "POST_contracts_publish_3.0.0_feat.json",
    )
    .await;
    let mut with_build = request("Consumer", "3.0.1", Some("feat"), None, vec![c1.clone()]);
    with_build["buildUrl"] = json!("https://ci.example.com/1");
    assert_publish_matches(
        &s,
        &with_build,
        "POST_contracts_publish_3.0.1_buildurl.json",
    )
    .await;
    let mut web_provider = pact();
    web_provider["consumer"]["name"] = json!("Web");
    let mut web_other = web_provider.clone();
    web_other["provider"]["name"] = json!("Other");
    assert_publish_matches(
        &s,
        &request(
            "Web",
            "1",
            Some("main"),
            Some(vec!["dev"]),
            vec![
                contract("Web", "Provider", &web_provider),
                contract("Web", "Other", &web_other),
            ],
        ),
        "POST_contracts_publish_two_contracts.json",
    )
    .await;

    // What the publishes left behind, compared with the version fixtures.
    for (path, name) in [
        (
            "/pacticipants/Consumer/versions/1.0.2",
            "GET_pacticipants_Consumer_versions_1.0.2.json",
        ),
        (
            "/pacticipants/Consumer/versions/3.0.1",
            "GET_pacticipants_Consumer_versions_3.0.1.json",
        ),
        (
            "/pacticipants/Web/versions/1",
            "GET_pacticipants_Web_versions_1.json",
        ),
    ] {
        let got: Value = s
            .client
            .get(format!("{b}{path}"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(stable(got), phase2(name, b), "{path}");
    }
    let buildurl: Value = s
        .client
        .get(format!("{b}/pacticipants/Consumer/versions/3.0.1"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(buildurl["buildUrl"], "https://ci.example.com/1");
}

#[tokio::test]
async fn publish_contracts_reports_a_content_change_against_the_last_tagged_version() {
    // Not captured: the Ruby broker's wording for a changed pact on an existing tag is
    // assumed from its source, "pact content has changed since the last consumer version
    // tagged with {tag}".
    let s = common::start().await;
    let rename = |mut p: Value| {
        p["consumer"]["name"] = json!("C");
        p["provider"]["name"] = json!("P");
        p
    };
    let (p1, p2) = (rename(pact()), rename(changed_pact()));
    let r = publish(
        &s,
        &request(
            "C",
            "1",
            None,
            Some(vec!["dev"]),
            vec![contract("C", "P", &p1)],
        ),
    )
    .await;
    assert_eq!(r.status(), StatusCode::OK);
    let r = publish(
        &s,
        &request(
            "C",
            "2",
            None,
            Some(vec!["dev"]),
            vec![contract("C", "P", &p2)],
        ),
    )
    .await;
    let body: Value = r.json().await.unwrap();
    assert!(notices(&body).iter().any(|n| n.ends_with("Events detected: contract_published, contract_content_changed (pact content has changed since the last consumer version tagged with dev)")), "{body}");
    let r = publish(
        &s,
        &request(
            "C",
            "3",
            None,
            Some(vec!["dev"]),
            vec![contract("C", "P", &p2)],
        ),
    )
    .await;
    let body: Value = r.json().await.unwrap();
    assert!(notices(&body).iter().any(|n| n.ends_with("Events detected: contract_published (pact content is the same as previous versions with tags dev and no new tags were applied)")), "{body}");
}

#[tokio::test]
async fn publish_contracts_validation_errors_match_the_ruby_broker() {
    let s = common::start().await;
    let b = &s.base_url;
    let c1 = contract("Consumer", "Provider", &pact());
    let mut web = pact();
    web["consumer"]["name"] = json!("Web");
    let cases: Vec<(Value, &str)> = vec![
        (json!({}), "POST_contracts_publish_err_empty.json"),
        (
            request(
                "Consumer",
                "9",
                None,
                None,
                vec![contract("Web", "Provider", &pact())],
            ),
            "POST_contracts_publish_err_consumer_mismatch.json",
        ),
        (
            json!({"pacticipantName":"Consumer","pacticipantVersionNumber":"9","contracts":[{"consumerName":"Consumer","providerName":"Provider","specification":"pact","contentType":"application/json","content":"!!!notbase64"}]}),
            "POST_contracts_publish_err_bad_base64.json",
        ),
        (
            json!({"pacticipantName":"Consumer","pacticipantVersionNumber":"9","contracts":[{"consumerName":"Consumer","providerName":"Provider","specification":"pact","contentType":"application/json","content": base64::engine::general_purpose::STANDARD.encode("not json")}]}),
            "POST_contracts_publish_err_bad_json_content.json",
        ),
        (
            request(
                "Consumer",
                "9",
                None,
                None,
                vec![contract("Consumer", "Provider", &web)],
            ),
            "POST_contracts_publish_err_content_names_differ.json",
        ),
        (
            json!({"pacticipantName":"Consumer","pacticipantVersionNumber":" ","contracts":[]}),
            "POST_contracts_publish_err_whitespace_version.json",
        ),
        (
            json!({"pacticipantName":1,"pacticipantVersionNumber":"9","tags":"dev","branch":["x"],"contracts":[{"consumerName":"Consumer","providerName":"Provider","specification":"openapi","contentType":"text/plain","content":"aGk="}]}),
            "POST_contracts_publish_err_bad_types.json",
        ),
        (
            json!({"pacticipantName":"Consumer","pacticipantVersionNumber":"9","contracts":[{"consumerName":"Consumer","providerName":"Provider","specification":"pact","contentType":"application/xml","content": c1["content"]}]}),
            "POST_contracts_publish_err_wrong_content_type.json",
        ),
    ];
    for (body, name) in cases {
        let r = publish(&s, &body).await;
        assert_eq!(r.status(), StatusCode::BAD_REQUEST, "{name}");
        assert_eq!(content_type(&r), HAL, "{name}");
        let got: Value = r.json().await.unwrap();
        assert_eq!(got, phase2(name, b), "{name}");
    }
    // The Ruby broker answers an empty contracts array with a 500; pactling validates it.
    let r = publish(
        &s,
        &json!({"pacticipantName":"Consumer","pacticipantVersionNumber":"9","contracts":[]}),
    )
    .await;
    assert_eq!(r.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        r.json::<Value>().await.unwrap(),
        json!({"errors": {"contracts": ["cannot be empty"]}})
    );
    let r = publish_raw(&s, "nope").await;
    assert_eq!(r.status(), StatusCode::BAD_REQUEST);
    assert!(
        r.json::<Value>().await.unwrap()["error"]
            .as_str()
            .unwrap()
            .starts_with("JSON::ParserError - ")
    );
    assert_eq!(
        s.client
            .get(format!("{b}/pacticipants/Consumer"))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND,
        "nothing was created by failed publishes"
    );
}

#[tokio::test]
async fn publish_contracts_methods() {
    let s = common::start().await;
    for m in [reqwest::Method::GET, reqwest::Method::PUT] {
        let r = s
            .client
            .request(m.clone(), format!("{}/contracts/publish", s.base_url))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::METHOD_NOT_ALLOWED, "{m}");
        assert_eq!(r.headers()["allow"], "POST, OPTIONS");
    }
    let r = s
        .client
        .request(
            reqwest::Method::OPTIONS,
            format!("{}/contracts/publish", s.base_url),
        )
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    assert_eq!(r.headers()["access-control-allow-methods"], "POST, OPTIONS");
}

#[tokio::test]
async fn legacy_put_publish_sequence_matches_the_ruby_broker() {
    let s = common::start().await;
    let b = &s.base_url;
    let p1 = serde_json::to_string(&pact()).unwrap();

    let r = put_pact(&s, "Provider", "Consumer", "4.0.0", &p1).await;
    assert_eq!(r.status(), StatusCode::CREATED);
    assert_eq!(content_type(&r), HAL);
    let got: Value = r.json().await.unwrap();
    assert_eq!(stable(got), phase2("PUT_pact_4.0.0_new.json", b));

    let r = put_pact(&s, "Provider", "Consumer", "4.0.0", &p1).await;
    assert_eq!(r.status(), StatusCode::OK);
    assert_eq!(
        stable(r.json().await.unwrap()),
        phase2("PUT_pact_4.0.0_same.json", b)
    );

    let r = put_pact(
        &s,
        "Provider",
        "Consumer",
        "4.0.0",
        &serde_json::to_string(&changed_pact()).unwrap(),
    )
    .await;
    assert_eq!(r.status(), StatusCode::CONFLICT);
    assert_eq!(content_type(&r), HAL);
    assert_eq!(
        r.json::<Value>().await.unwrap(),
        phase2("PUT_pact_4.0.0_changed.json", b)
    );

    let r = put_pact(&s, "Provider", "Consumer", "4.0.1", "nope").await;
    assert_eq!(r.status(), StatusCode::BAD_REQUEST);
    assert!(
        r.json::<Value>().await.unwrap()["error"]
            .as_str()
            .unwrap()
            .starts_with("JSON::ParserError - ")
    );

    let minimal = r#"{"consumer":{"name":"Consumer"},"provider":{"name":"Provider"},"interactions":[],"metadata":{}}"#;
    let r = put_pact(&s, "Provider", "Consumer", "4.0.2", minimal).await;
    assert_eq!(r.status(), StatusCode::CREATED);
    assert_eq!(
        stable(r.json().await.unwrap()),
        phase2("PUT_pact_4.0.2_minimal.json", b)
    );

    let r = put_pact(
        &s,
        "Provider",
        "Consumer",
        "4.0.3",
        r#"{"consumer":{"name":"Other"},"provider":{"name":"Provider"},"interactions":[]}"#,
    )
    .await;
    assert_eq!(r.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        r.json::<Value>().await.unwrap(),
        phase2("PUT_pact_mismatch.json", b)
    );

    let r = put_pact(
        &s,
        "Provider",
        "Consumer",
        "4.0.4",
        r#"{"interactions":[]}"#,
    )
    .await;
    assert_eq!(
        r.status(),
        StatusCode::CREATED,
        "missing names are filled from the URL"
    );
    assert_eq!(
        stable(r.json().await.unwrap()),
        phase2("PUT_pact_4.0.4_nonames.json", b)
    );

    let r = put_pact(&s, "Provider", "consumer", "1", minimal).await;
    assert_eq!(
        r.status(),
        StatusCode::BAD_REQUEST,
        "name matching is case-sensitive"
    );
    assert_eq!(
        r.json::<Value>().await.unwrap(),
        phase2("PUT_pact_duplicate_name_case.json", b)
    );

    let r = put_pact(&s, "Provider", "Consumer", "1.0.0%2Bbuild", minimal).await;
    assert_eq!(r.status(), StatusCode::CREATED);
    assert_eq!(
        stable(r.json().await.unwrap()),
        phase2("PUT_pact_1.0.0%2Bbuild.json", b)
    );

    let three = r#"{"consumer":{"name":"Consumer"},"provider":{"name":"Provider"},"interactions":[{"description":"zeta","request":{"method":"GET","path":"/a"},"response":{"status":200}},{"description":"alpha","providerState":"zz","request":{"method":"GET","path":"/b"},"response":{"status":200}},{"description":"mid","request":{"method":"POST","path":"/c"},"response":{"status":201}}],"metadata":{"pactSpecification":{"version":"2.0.0"}}}"#;
    let r = put_pact(&s, "Provider", "Consumer", "7.0.0", three).await;
    assert_eq!(r.status(), StatusCode::CREATED);
    assert_eq!(
        stable(r.json().await.unwrap()),
        phase2("PUT_pact_7.0.0_three_interactions.json", b)
    );

    // Version numbers in the URL are decoded; the version document link encodes them back.
    let v: Value = s
        .client
        .get(format!("{b}/pacticipants/Consumer/versions/1.0.0%2Bbuild"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(v["number"], "1.0.0+build");
}

#[tokio::test]
async fn pact_resource_methods_and_delete() {
    let s = common::start().await;
    let b = &s.base_url;
    put_pact(
        &s,
        "Provider",
        "Consumer",
        "1.0.0",
        &serde_json::to_string(&pact()).unwrap(),
    )
    .await;
    let url = format!("{b}/pacts/provider/Provider/consumer/Consumer/version/1.0.0");
    let r = s.client.head(&url).send().await.unwrap();
    assert_eq!(r.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(r.headers()["allow"], "GET, PUT, DELETE, PATCH, OPTIONS");
    let r = s
        .client
        .request(reqwest::Method::OPTIONS, &url)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    assert_eq!(
        r.headers()["access-control-allow-methods"],
        "GET, PUT, DELETE, PATCH, OPTIONS"
    );
    let r = s.client.patch(&url).send().await.unwrap();
    assert_eq!(r.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
    let r = s.client.put(&url).body("{}").send().await.unwrap();
    assert_eq!(
        r.status(),
        StatusCode::UNSUPPORTED_MEDIA_TYPE,
        "PUT without a JSON content type"
    );

    let r = s.client.delete(&url).send().await.unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    assert_eq!(content_type(&r), HAL);
    assert_eq!(
        r.json::<Value>().await.unwrap(),
        phase2("DELETE_pact_1.0.0.json", b)
    );
    let r = s.client.delete(&url).send().await.unwrap();
    assert_eq!(r.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        r.json::<Value>().await.unwrap(),
        json!({"error": "The requested document was not found on this server."})
    );
    // The consumer version survives; only the publication is gone.
    assert_eq!(
        s.client
            .get(format!("{b}/pacticipants/Consumer/versions/1.0.0"))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
}
