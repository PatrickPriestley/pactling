mod common;
use pactling::config::Config;
use serde_json::{Value, json};
const ROUTE: &str = "/pactling/v1/can-i-deploy/explanation";
const QUERY: &str = "pacticipant=C&version=1&environment=test&latestby=cvp";

async fn seed(s: &common::TestServer, client: &reqwest::Client, success: Option<bool>) {
    let pact: Value = client
        .put(format!(
            "{}/pacts/provider/P/consumer/C/version/1",
            s.base_url
        ))
        .json(&json!({"consumer":{"name":"C"},"provider":{"name":"P"},"interactions":[]}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    client
        .put(format!("{}/pacticipants/P/versions/2", s.base_url))
        .json(&json!({}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    let env: Value = client
        .post(format!("{}/environments", s.base_url))
        .json(&json!({"name":"test","production":false}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    client
        .post(format!(
            "{}/pacticipants/P/versions/2/deployed-versions/environment/{}",
            s.base_url,
            env["uuid"].as_str().unwrap()
        ))
        .json(&json!({}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    if let Some(success) = success {
        client
            .post(
                pact["_links"]["pb:publish-verification-results"]["href"]
                    .as_str()
                    .unwrap(),
            )
            .json(&json!({"providerApplicationVersion":"2","success":success}))
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap();
    }
}

#[tokio::test]
async fn extension_maps_allowed_blocked_unknown_without_changing_legacy_documents() {
    for (success, decision) in [
        (Some(true), "allowed"),
        (Some(false), "blocked"),
        (None, "unknown"),
    ] {
        let s = common::start().await;
        seed(&s, &s.client, success).await;
        let paths = [
            "/".to_string(),
            "/index".to_string(),
            format!("/matrix?{QUERY}"),
            format!("/can-i-deploy?{QUERY}"),
        ];
        let mut before = vec![];
        for path in &paths {
            let r = s
                .client
                .get(format!("{}{path}", s.base_url))
                .send()
                .await
                .unwrap();
            before.push((r.status(), r.text().await.unwrap()));
        }
        let r = s
            .client
            .get(format!("{}{ROUTE}?{QUERY}", s.base_url))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 200, "missing explanation route for {decision}");
        assert_eq!(r.headers()["content-type"], "application/json");
        assert_eq!(r.headers()["cache-control"], "no-store");
        let d: Value = r.json().await.unwrap();
        assert_eq!(d["decision"], decision);
        assert_eq!(d["schemaVersion"], "1.0");
        assert_eq!(d["compatibility"]["deployable"], json!(success));
        assert_eq!(d["evidence"]["currentness"], "not_checked");
        assert!(d["evidence"].get("freshness").is_none());
        assert_eq!(d["request"]["query"], QUERY);
        assert_eq!(
            d["rows"][0]["providerVersion"],
            if success.is_some() {
                json!("2")
            } else {
                Value::Null
            }
        );
        if success.is_none() {
            assert_eq!(d["nextActions"][0]["type"], "VERIFY_CONTRACT");
            assert_eq!(d["nextActions"][0]["target"]["providerVersion"], "2");
        }
        for (path, expected) in paths.iter().zip(before) {
            let r = s
                .client
                .get(format!("{}{path}", s.base_url))
                .send()
                .await
                .unwrap();
            assert_eq!((r.status(), r.text().await.unwrap()), expected, "{path}");
        }
    }
}

#[tokio::test]
async fn explanation_validation_envelope_preserves_remaining_required_fields() {
    let s = common::start().await;
    seed(&s, &s.client, None).await;
    for query in [
        "",
        "pacticipant=C",
        "pacticipant=C&version=1",
        "pacticipant=NoSuch&version=1&environment=test",
        "pacticipant=C&version=1&environment=absent",
    ] {
        let legacy = s
            .client
            .get(format!("{}/can-i-deploy?{query}", s.base_url))
            .send()
            .await
            .unwrap();
        assert_eq!(legacy.status(), 400);
        let mut legacy: Value = legacy.json().await.unwrap();
        // The explanation no longer requires a pin; the compatibility route still does.
        if let Some(errors) = legacy["errors"].as_object_mut() {
            errors.remove("version");
        }
        let r = s
            .client
            .get(format!("{}{ROUTE}?{query}", s.base_url))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 400);
        assert_eq!(r.headers()["content-type"], "application/json");
        assert_eq!(r.headers()["cache-control"], "no-store");
        let d: Value = r.json().await.unwrap();
        assert_eq!(
            d,
            json!({"schemaVersion":"1.0","error":{"code":"INVALID_QUERY","details":legacy["errors"]}})
        );
        assert!(d.get("decision").is_none());
    }
}

fn authenticated_client(user: &str, password: &str) -> reqwest::Client {
    use base64::Engine;
    let mut h = reqwest::header::HeaderMap::new();
    h.insert(
        "authorization",
        format!(
            "Basic {}",
            base64::engine::general_purpose::STANDARD.encode(format!("{user}:{password}"))
        )
        .parse()
        .unwrap(),
    );
    reqwest::Client::builder()
        .default_headers(h)
        .build()
        .unwrap()
}

#[tokio::test]
async fn extension_is_read_only_and_auth_protected() {
    let mut c = Config::in_memory();
    c.basic_auth_username = Some("writer".into());
    c.basic_auth_password = Some("write-test".into());
    c.basic_auth_read_only_username = Some("reader".into());
    c.basic_auth_read_only_password = Some("read-test".into());
    let s = common::start_with(c).await;
    let writer = authenticated_client("writer", "write-test");
    seed(&s, &writer, Some(true)).await;
    let reader = authenticated_client("reader", "read-test");
    let url = format!("{}{ROUTE}?{QUERY}", s.base_url);
    assert_eq!(s.client.get(&url).send().await.unwrap().status(), 401);
    assert_eq!(
        authenticated_client("reader", "wrong")
            .get(&url)
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    assert_eq!(reader.get(&url).send().await.unwrap().status(), 200);
    assert_eq!(reader.post(&url).send().await.unwrap().status(), 401);
    assert_eq!(writer.post(&url).send().await.unwrap().status(), 405);
}

#[tokio::test]
async fn methods_and_no_store_headers_and_no_matrix_extension() {
    let s = common::start().await;
    let url = format!("{}{ROUTE}", s.base_url);
    let r = s
        .client
        .request(reqwest::Method::OPTIONS, &url)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    assert_eq!(r.headers()["access-control-allow-methods"], "GET, OPTIONS");
    for method in [
        reqwest::Method::POST,
        reqwest::Method::PUT,
        reqwest::Method::DELETE,
        reqwest::Method::HEAD,
    ] {
        let r = s.client.request(method, &url).send().await.unwrap();
        assert_eq!(r.status(), 405);
        assert_eq!(r.headers()["allow"], "GET, OPTIONS");
    }
    assert_eq!(
        s.client
            .get(format!("{}/pactling/v1/matrix/explanation", s.base_url))
            .send()
            .await
            .unwrap()
            .status(),
        404
    );
    assert_eq!(
        s.client
            .get(format!(
                "{}/pactling/v2/can-i-deploy/explanation",
                s.base_url
            ))
            .send()
            .await
            .unwrap()
            .status(),
        404
    );
}

#[tokio::test]
async fn internal_errors_have_no_decision_or_database_details() {
    let path = std::env::temp_dir().join(format!("explanation-error-{}.db", uuid::Uuid::new_v4()));
    let mut config = Config::in_memory();
    config.database_url = format!("sqlite://{}?mode=rwc", path.display());
    let server = common::start_with(config.clone()).await;
    let pool = sqlx::SqlitePool::connect(&config.database_url)
        .await
        .unwrap();
    sqlx::query("DROP TABLE pacticipants")
        .execute(&pool)
        .await
        .unwrap();
    let response = server
        .client
        .get(format!("{}{ROUTE}?{QUERY}", server.base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 500);
    assert_eq!(response.headers()["cache-control"], "no-store");
    assert_eq!(
        response.json::<Value>().await.unwrap(),
        json!({"error":"internal error"})
    );
    pool.close().await;
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn unpinned_main_branch_explanation_matches_matrix_without_changing_can_i_deploy() {
    let s = common::start().await;
    seed(&s, &s.client, Some(true)).await;
    s.client
        .put(format!(
            "{}/pacticipants/C/versions/1/tags/main",
            s.base_url
        ))
        .json(&json!({}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    let query =
        "q[][pacticipant]=C&q[][latest]=true&q[][mainBranch]=true&latestby=cvp&environment=test";
    let matrix: Value = s
        .client
        .get(format!("{}/matrix?{query}", s.base_url))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(matrix["summary"]["deployable"], true);
    let old = s
        .client
        .get(format!("{}/can-i-deploy?{query}", s.base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(old.status(), 400);
    assert_eq!(
        old.json::<Value>().await.unwrap()["errors"],
        json!({"version":["is missing"]})
    );
    let response = s
        .client
        .get(format!("{}{ROUTE}?{query}", s.base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        200,
        "real CLI main-branch query must be accepted by explanation only"
    );
    let explanation: Value = response.json().await.unwrap();
    assert_eq!(explanation["decision"], "allowed");
    assert_eq!(
        explanation["compatibility"]["deployable"],
        matrix["summary"]["deployable"]
    );
    assert_eq!(explanation["request"]["query"], query);
}

#[tokio::test]
async fn explanation_accepts_all_matrix_selector_forms_with_required_target() {
    let s = common::start().await;
    seed(&s, &s.client, Some(true)).await;
    s.client
        .put(format!(
            "{}/pacticipants/C/versions/1/tags/main",
            s.base_url
        ))
        .json(&json!({}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    for selector in [
        "",
        "&q[][version]=1",
        "&q[][latest]=true",
        "&q[][latest]=true&q[][tag]=main",
        "&q[][latest]=true&q[][branch]=main",
        "&q[][latest]=true&q[][mainBranch]=true",
    ] {
        for target in ["environment=test", "latest=true&tag=main", "to=main"] {
            let query = format!("q[][pacticipant]=C{selector}&latestby=cvp&{target}");
            let matrix: Value = s
                .client
                .get(format!("{}/matrix?{query}", s.base_url))
                .send()
                .await
                .unwrap()
                .error_for_status()
                .unwrap()
                .json()
                .await
                .unwrap();
            let response = s
                .client
                .get(format!("{}{ROUTE}?{query}", s.base_url))
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), 200, "{query}");
            let explanation: Value = response.json().await.unwrap();
            for key in ["deployable", "success", "failed", "unknown"] {
                assert_eq!(
                    explanation["compatibility"][key], matrix["summary"][key],
                    "{query}: {key}"
                );
            }
            assert_eq!(explanation["request"]["query"], query);
        }
    }
}
