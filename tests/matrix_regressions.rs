//! Deployment-gate regressions, verified against Ruby broker 2.121.2.
mod common;
use common::TestServer;
use serde_json::{Value, json};

async fn publish(s: &TestServer, consumer: &str, provider: &str, success: bool) {
    let pact: Value = s
        .client
        .put(format!(
            "{}/pacts/provider/{provider}/consumer/{consumer}/version/1",
            s.base_url
        ))
        .json(&json!({"consumer":{"name":consumer},"provider":{"name":provider},"interactions":[]}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    s.client
        .post(
            pact["_links"]["pb:publish-verification-results"]["href"]
                .as_str()
                .unwrap(),
        )
        .json(&json!({"providerApplicationVersion":"1","success":success}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
}

async fn matrix(s: &TestServer, selectors: &[(&str, &str)], ignore: Option<(&str, &str)>) -> Value {
    let mut query = vec![];
    for (p, v) in selectors {
        query.extend([("q[][pacticipant]", *p), ("q[][version]", *v)]);
    }
    query.push(("latestby", "cvpv"));
    if let Some((p, v)) = ignore {
        query.extend([("ignore[][pacticipant]", p), ("ignore[][version]", v)]);
    }
    let query = query
        .iter()
        .map(|(k, v)| {
            format!(
                "{}={}",
                pactling::http::hal::encode(k),
                pactling::http::hal::encode(v)
            )
        })
        .collect::<Vec<_>>()
        .join("&");
    s.client
        .get(format!("{}/matrix?{query}", s.base_url))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap()
}

#[tokio::test]
async fn selector_order_cannot_hide_a_failed_integration() {
    let s = common::start().await;
    publish(&s, "C", "P", false).await;
    let forward = matrix(&s, &[("C", "1"), ("P", "1")], None).await;
    let reverse = matrix(&s, &[("P", "1"), ("C", "1")], None).await;
    assert_eq!(forward["summary"]["deployable"], false);
    assert_eq!(reverse, forward);
}

#[tokio::test]
async fn third_selector_is_evaluated_in_every_permutation() {
    let s = common::start().await;
    publish(&s, "C", "P", false).await;
    publish(&s, "W", "P", true).await;
    for names in [
        ["W", "P", "C"],
        ["C", "W", "P"],
        ["P", "C", "W"],
        ["W", "C", "P"],
        ["C", "P", "W"],
        ["P", "W", "C"],
    ] {
        let q: Vec<_> = names.iter().map(|n| (*n, "1")).collect();
        let result = matrix(&s, &q, None).await;
        assert_eq!(result["summary"]["deployable"], false, "{names:?}");
        assert_eq!(result["summary"]["failed"], 1);
        assert_eq!(result["summary"]["success"], 1);
        assert_eq!(result["matrix"].as_array().unwrap().len(), 2);
    }
}

#[tokio::test]
async fn missing_pinned_version_never_reuses_another_versions_success() {
    let s = common::start().await;
    publish(&s, "C", "P", true).await;
    for selectors in [[("C", "1"), ("P", "999")], [("P", "999"), ("C", "1")]] {
        let result = matrix(&s, &selectors, None).await;
        assert_eq!(result["summary"]["deployable"], false);
        assert_eq!(result["summary"]["success"], 0);
        assert_eq!(result["summary"]["unknown"], 1);
        assert_eq!(
            result["summary"]["reason"],
            "No pacts or verifications have been published for version 999 of P"
        );
    }
}

#[tokio::test]
async fn ignore_matches_the_version_not_just_the_pacticipant() {
    let s = common::start().await;
    publish(&s, "C", "P", false).await;
    let wrong = matrix(&s, &[("C", "1"), ("P", "1")], Some(("P", "999"))).await;
    assert_eq!(wrong["summary"]["deployable"], false);
    assert_eq!(wrong["summary"]["ignored"], 0);
    let right = matrix(&s, &[("P", "1"), ("C", "1")], Some(("P", "1"))).await;
    assert_eq!(right["summary"]["deployable"], true);
    assert_eq!(right["summary"]["ignored"], 1);
}

#[tokio::test]
async fn captured_matrix_regressions_match_ruby() {
    let capture: Value = serde_json::from_str(include_str!(
        "fixtures/ruby-broker/2.121.2/phase4/matrix-regressions.json"
    ))
    .unwrap();
    assert_eq!(capture["brokerVersion"], "2.121.2");
    let s = common::start().await;
    let mut variables = std::collections::HashMap::<String, String>::new();
    for op in capture["setup"].as_array().unwrap() {
        let mut path = op["path"].as_str().unwrap().to_owned();
        for (name, value) in &variables {
            path = path.replace(&format!("{{{name}}}"), value);
        }
        let response = s
            .client
            .request(
                op["method"].as_str().unwrap().parse().unwrap(),
                format!("{}{path}", s.base_url),
            )
            .json(&op["body"])
            .send()
            .await
            .unwrap();
        assert!(
            response.status().is_success(),
            "setup {path}: {}",
            response.status()
        );
        if let Some(name) = op["saveUuid"].as_str() {
            let body: Value = response.json().await.unwrap();
            variables.insert(name.to_owned(), body["uuid"].as_str().unwrap().to_owned());
        }
    }
    let mut mismatches = vec![];
    for case in capture["cases"].as_array().unwrap() {
        let response = s
            .client
            .get(format!(
                "{}{}",
                s.base_url,
                case["request"]["path"].as_str().unwrap()
            ))
            .send()
            .await
            .unwrap();
        assert_eq!(
            response.status().as_u16(),
            case["status"].as_u64().unwrap() as u16
        );
        let body: Value = response.json().await.unwrap();
        let rows: Vec<_> = body["matrix"].as_array().unwrap().iter().map(|r| json!({
            "consumer":r["consumer"]["name"], "consumerVersion":r["consumer"]["version"]["number"],
            "provider":r["provider"]["name"], "providerVersion":r["provider"]["version"]["number"],
            "success":r["verificationResult"]["success"], "ignored":r.get("ignored").cloned().unwrap_or(json!(false))
        })).collect();
        let mut summary = body["summary"].clone();
        // This explicitly scoped control establishes that Ruby's integration route
        // is bidirectional. Existing unpinned reason wording is outside this fix.
        if case["compareReason"] == false {
            summary.as_object_mut().unwrap().shift_remove("reason");
        }
        let actual = json!({"summary":summary,"rows":rows});
        if actual != case["expected"] {
            mismatches.push(format!(
                "{}: actual {} expected {}",
                case["name"], actual, case["expected"]
            ));
        }
    }
    assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
}

#[tokio::test]
async fn missing_consumer_pin_is_negative_in_both_orders() {
    let s = common::start().await;
    publish(&s, "C", "P", true).await;
    for selectors in [[("C", "999"), ("P", "1")], [("P", "1"), ("C", "999")]] {
        let result = matrix(&s, &selectors, None).await;
        assert_eq!(result["summary"]["deployable"], false);
        assert!(
            result["summary"]["reason"]
                .as_str()
                .unwrap()
                .contains("version 999 of C")
        );
    }
}

#[tokio::test]
async fn missing_latest_branch_cannot_approve_a_deployment() {
    let s = common::start().await;
    publish(&s, "C", "P", true).await;
    // Ruby 2.121.2 returns false for this exact selector shape, probed separately.
    let response: Value = s.client.get(format!("{}/matrix?q%5B%5D%5Bpacticipant%5D=C&q%5B%5D%5Blatest%5D=true&q%5B%5D%5Bbranch%5D=absent&q%5B%5D%5Bpacticipant%5D=P&q%5B%5D%5Bversion%5D=1&latestby=cvpv", s.base_url))
        .send().await.unwrap().error_for_status().unwrap().json().await.unwrap();
    assert_eq!(response["summary"]["deployable"], false);
}
