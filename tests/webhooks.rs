//! Webhooks, compared with the phase 5 fixtures captured from Ruby broker 2.121.2 by
//! replaying the capture's sequence. The CLI's own exchanges are replayed from the
//! recorded proxy traffic in tests/fixtures/cli/phase5/.
mod common;
use base64::Engine;
use common::{fixture, stable};
use pactling::config::Config;
use reqwest::StatusCode;
use serde_json::{Value, json};

const HAL: &str = "application/hal+json;charset=utf-8";

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
    tags: Vec<&str>,
    content: &Value,
) -> Value {
    let body = json!({ "pacticipantName": consumer, "pacticipantVersionNumber": version, "branch": "main", "tags": tags, "contracts": [contract(consumer, content)] });
    let r = s
        .client
        .post(format!("{}/contracts/publish", s.base_url))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert!(r.status().is_success(), "{}", r.status());
    r.json().await.unwrap()
}

async fn request(
    s: &common::TestServer,
    method: reqwest::Method,
    path: &str,
    body: Option<&Value>,
) -> reqwest::Response {
    let mut r = s
        .client
        .request(method, format!("{}{path}", s.base_url))
        .header("accept", "application/hal+json");
    if let Some(b) = body {
        r = r.header("content-type", "application/json").json(b);
    }
    r.send().await.unwrap()
}

async fn get(s: &common::TestServer, path: &str) -> reqwest::Response {
    request(s, reqwest::Method::GET, path, None).await
}

async fn post(s: &common::TestServer, path: &str, body: &Value) -> reqwest::Response {
    request(s, reqwest::Method::POST, path, Some(body)).await
}

/// Webhook uuids (22 url-safe characters), triggered-webhook uuids, dates in logs, the
/// receiver host and verification numbers differ between brokers.
fn normalise(v: Value) -> Value {
    let text = serde_json::to_string(&v).unwrap();
    let text = regex::Regex::new(r"webhooks/[A-Za-z0-9_-]{22}")
        .unwrap()
        .replace_all(&text, "webhooks/UUID");
    let text = regex::Regex::new(r#""uuid":"[A-Za-z0-9_-]{22}""#)
        .unwrap()
        .replace_all(&text, r#""uuid":"UUID""#);
    let text = regex::Regex::new(r"[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}")
        .unwrap()
        .replace_all(&text, "UUID");
    let text = regex::Regex::new(r"\[\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z\]")
        .unwrap()
        .replace_all(&text, "[T]");
    let text = regex::Regex::new(r#""(triggeredAt|createdAt)":"[^"]*""#)
        .unwrap()
        .replace_all(&text, r#""$1":"T""#);
    serde_json::from_str(&text).unwrap()
}

fn phase5(name: &str, base: &str) -> Value {
    normalise(stable(fixture(&format!("phase5/{name}"), base)))
}

async fn assert_matches(
    s: &common::TestServer,
    r: reqwest::Response,
    name: &str,
    status: StatusCode,
) -> Value {
    assert_eq!(r.status(), status, "{name}");
    let got: Value = r.json().await.unwrap();
    let (left, right) = (normalise(stable(got.clone())), phase5(name, &s.base_url));
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

/// One request/response pair the CLI made, from the proxy recording.
struct Exchange {
    method: String,
    path: String,
    body: Value,
    status: u16,
    response: Value,
}

fn cli_exchange(file: &str, method: &str) -> Exchange {
    let path = format!(
        "{}/tests/fixtures/cli/phase5/{file}.jsonl",
        env!("CARGO_MANIFEST_DIR")
    );
    let raw = std::fs::read_to_string(&path).unwrap();
    for line in raw.lines() {
        let e: Value = serde_json::from_str(line).unwrap();
        if e["request"]["method"] == method {
            return Exchange {
                method: method.to_owned(),
                path: e["request"]["path"].as_str().unwrap().to_owned(),
                body: serde_json::from_str(e["request"]["body"].as_str().unwrap()).unwrap(),
                status: e["response"]["status"].as_u64().unwrap() as u16,
                response: serde_json::from_str(e["response"]["body"].as_str().unwrap()).unwrap(),
            };
        }
    }
    panic!("no {method} in {file}");
}

/// Replays what the CLI sent and checks the answer against what the Ruby broker gave.
/// The CLI talked to the Ruby broker through a proxy, so hosts are normalised on both sides.
async fn replay_cli(s: &common::TestServer, file: &str, method: &str) -> Value {
    let e = cli_exchange(file, method);
    let r = request(s, e.method.parse().unwrap(), &e.path, Some(&e.body)).await;
    assert_eq!(r.status().as_u16(), e.status, "{file}");
    let got: Value = r.json().await.unwrap();
    let hosts = regex::Regex::new(r"http://(127\.0\.0\.1|localhost):\d+").unwrap();
    let strip = |v: &Value| -> Value {
        let text = hosts
            .replace_all(&serde_json::to_string(v).unwrap(), "http://HOST")
            .into_owned();
        normalise(stable(serde_json::from_str(&text).unwrap()))
    };
    assert_eq!(strip(&got), strip(&e.response), "{file}");
    got
}

fn http_config() -> Config {
    let mut c = Config::in_memory();
    c.webhook_scheme_whitelist = vec!["http".into()];
    c
}

/// The capture's seed: pacts for Consumer and Web, Provider 1 verifying both on main, an
/// environment with Provider 1 deployed, and Provider 2 as the main branch head.
async fn seed(s: &common::TestServer) {
    publish(
        s,
        "Consumer",
        "1.0.0",
        vec!["dev"],
        &pact("Consumer", |_| {}),
    )
    .await;
    publish(s, "Web", "1", vec![], &pact("Web", |_| {})).await;
    for consumer_path in ["Consumer/version/1.0.0", "Web/version/1"] {
        let r = s
            .client
            .put(format!(
                "{}/pacticipants/Provider/branches/main/versions/1",
                s.base_url
            ))
            .header("content-type", "application/json")
            .body("{}")
            .send()
            .await
            .unwrap();
        assert!(r.status().is_success());
        let doc: Value = get(
            s,
            &format!("/pacts/provider/Provider/consumer/{consumer_path}"),
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
            .json(&json!({"providerApplicationVersion": "1", "success": true}))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::CREATED);
    }
    let env: Value = post(
        s,
        "/environments",
        &json!({"name": "test", "production": false}),
    )
    .await
    .json()
    .await
    .unwrap();
    let r = post(
        s,
        &format!(
            "/pacticipants/Provider/versions/1/deployed-versions/environment/{}",
            env["uuid"].as_str().unwrap()
        ),
        &json!({}),
    )
    .await;
    assert_eq!(r.status(), StatusCode::CREATED);
    let r = s
        .client
        .put(format!(
            "{}/pacticipants/Provider/branches/main/versions/2",
            s.base_url
        ))
        .header("content-type", "application/json")
        .body("{}")
        .send()
        .await
        .unwrap();
    assert!(r.status().is_success());
}

#[tokio::test]
async fn webhook_documents_match_the_ruby_broker() {
    let s = common::start_with(http_config()).await;
    seed(&s).await;
    // The CLI's create-webhook posts to the scoped collection; the answers are its own.
    replay_cli(&s, "create-webhook-consumer-provider", "POST").await;
    replay_cli(&s, "create-webhook-provider-all-events", "POST").await;
    replay_cli(&s, "create-webhook-bad-method", "POST").await;
    replay_cli(&s, "create-webhook-https", "POST").await;
    replay_cli(&s, "create-webhook-unknown-consumer", "POST").await;
    replay_cli(&s, "create-or-update-webhook-create", "PUT").await;
    replay_cli(&s, "create-or-update-webhook-update", "PUT").await;
    let r = post(&s, "/webhooks", &json!({"request":{"method":"POST","url":"http://host.docker.internal:18100/hook/global","username":"hookuser","password":"hookpass","headers":{"X-Token":"abc","Accept":"application/json"},"body":{"template":"${pactbroker.consumerVersionNumber}"}}})).await;
    assert_eq!(r.headers()["content-type"], HAL);
    let global = assert_matches(&s, r, "POST_webhooks_global.json", StatusCode::CREATED).await;
    let global_uuid = global["uuid"].as_str().unwrap().to_owned();
    assert_eq!(global_uuid.len(), 22, "urlsafe base64 uuid: {global_uuid}");

    for (name, body) in [
        ("POST_webhooks_err_empty.json", json!({})),
        (
            "POST_webhooks_err_request_empty.json",
            json!({"request":{}}),
        ),
        (
            "POST_webhooks_err_method.json",
            json!({"request":{"method":"GET","url":"http://host.docker.internal:18100/hook"}}),
        ),
        (
            "POST_webhooks_err_scheme.json",
            json!({"request":{"method":"POST","url":"https://example.com/hook"}}),
        ),
        (
            "POST_webhooks_err_url.json",
            json!({"request":{"method":"POST","url":"nope"}}),
        ),
        (
            "POST_webhooks_err_templated_host.json",
            json!({"request":{"method":"POST","url":"http://${pactbroker.consumerName}.example.com/hook"}}),
        ),
        (
            "POST_webhooks_err_event.json",
            json!({"request":{"method":"POST","url":"http://host.docker.internal:18100/hook"},"events":[{"name":"bogus"}]}),
        ),
        (
            "POST_webhooks_err_events_empty.json",
            json!({"request":{"method":"POST","url":"http://host.docker.internal:18100/hook"},"events":[]}),
        ),
        (
            "POST_webhooks_err_unknown_consumer.json",
            json!({"consumer":{"name":"Nope"},"request":{"method":"POST","url":"http://host.docker.internal:18100/hook"}}),
        ),
        (
            "POST_webhooks_err_name_and_label.json",
            json!({"consumer":{"name":"Consumer","label":"x"},"request":{"method":"POST","url":"http://host.docker.internal:18100/hook"}}),
        ),
    ] {
        let r = post(&s, "/webhooks", &body).await;
        assert_matches(&s, r, name, StatusCode::BAD_REQUEST).await;
    }
    // Bad JSON: the message uses serde's wording (a deliberate difference, see README).
    let r = s
        .client
        .post(format!("{}/webhooks", s.base_url))
        .header("content-type", "application/json")
        .body("{nope")
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::BAD_REQUEST);
    assert_eq!(r.headers()["content-type"], HAL);
    let body: Value = r.json().await.unwrap();
    assert!(
        body["error"]
            .as_str()
            .unwrap()
            .starts_with("JSON::ParserError - "),
        "{body}"
    );
    let r = request(
        &s,
        reqwest::Method::PUT,
        "/webhooks/short",
        Some(&json!({"request":{"method":"POST","url":"http://host.docker.internal:18100/hook"}})),
    )
    .await;
    assert_matches(&s, r, "PUT_webhooks_err_uuid.json", StatusCode::BAD_REQUEST).await;

    let list = assert_matches(
        &s,
        get(&s, "/webhooks").await,
        "GET_webhooks.json",
        StatusCode::OK,
    )
    .await;
    let hrefs: Vec<String> = list["_links"]["pb:webhooks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|w| w["href"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(hrefs.len(), 4);
    let uuid_of = |i: usize| hrefs[i].rsplit('/').next().unwrap().to_owned();
    assert_matches(
        &s,
        get(&s, &format!("/webhooks/{}", uuid_of(0))).await,
        "GET_webhook_consumer_provider.json",
        StatusCode::OK,
    )
    .await;
    assert_matches(
        &s,
        get(&s, &format!("/webhooks/{}", uuid_of(1))).await,
        "GET_webhook_provider_all_events.json",
        StatusCode::OK,
    )
    .await;
    assert_matches(
        &s,
        get(&s, &format!("/webhooks/{global_uuid}")).await,
        "GET_webhook_global.json",
        StatusCode::OK,
    )
    .await;
    assert_matches(
        &s,
        get(&s, "/webhooks/pactling-capture-webhook-1").await,
        "GET_webhook_upsert.json",
        StatusCode::OK,
    )
    .await;
    assert_matches(
        &s,
        get(&s, "/webhooks/provider/Provider/consumer/Consumer").await,
        "GET_webhooks_provider_Provider_consumer_Consumer.json",
        StatusCode::OK,
    )
    .await;
    assert_matches(
        &s,
        get(&s, "/webhooks/provider/Provider").await,
        "GET_webhooks_provider_Provider.json",
        StatusCode::OK,
    )
    .await;
    assert_matches(
        &s,
        get(&s, "/webhooks/consumer/Consumer").await,
        "GET_webhooks_consumer_Consumer.json",
        StatusCode::OK,
    )
    .await;
    assert_matches(
        &s,
        get(&s, "/webhooks/consumer/Nope").await,
        "GET_webhooks_consumer_Nope.json",
        StatusCode::NOT_FOUND,
    )
    .await;
    assert_matches(
        &s,
        get(&s, "/pacts/provider/Provider/consumer/Consumer/webhooks").await,
        "GET_pact_webhooks.json",
        StatusCode::OK,
    )
    .await;
    assert_matches(
        &s,
        get(
            &s,
            "/pacts/provider/Provider/consumer/Consumer/webhooks/status",
        )
        .await,
        "GET_pact_webhooks_status_before.json",
        StatusCode::OK,
    )
    .await;
    assert_matches(
        &s,
        get(&s, "/pacts/provider/Provider/consumer/Web/webhooks/status").await,
        "GET_pact_webhooks_status_Web.json",
        StatusCode::OK,
    )
    .await;
    assert_matches(
        &s,
        get(
            &s,
            "/pacts/provider/Provider/consumer/Consumer/version/1.0.0/triggered-webhooks",
        )
        .await,
        "GET_triggered_webhooks_1.0.0_before.json",
        StatusCode::OK,
    )
    .await;
    for (path, allowed) in [
        ("/webhooks".to_owned(), "GET, POST, OPTIONS"),
        (
            format!("/webhooks/{global_uuid}"),
            "GET, PUT, DELETE, OPTIONS",
        ),
        (format!("/webhooks/{global_uuid}/execute"), "POST, OPTIONS"),
    ] {
        let r = request(&s, reqwest::Method::OPTIONS, &path, None).await;
        assert_eq!(
            r.headers()["access-control-allow-methods"],
            allowed,
            "{path}"
        );
    }
    let r = request(&s, reqwest::Method::DELETE, "/webhooks/nope", None).await;
    assert_matches(&s, r, "DELETE_webhook_unknown.json", StatusCode::NOT_FOUND).await;
    let r = request(
        &s,
        reqwest::Method::DELETE,
        &format!("/webhooks/{global_uuid}"),
        None,
    )
    .await;
    assert_eq!(r.status(), StatusCode::NO_CONTENT);
    let r = get(&s, &format!("/webhooks/{global_uuid}")).await;
    assert_matches(
        &s,
        r,
        "GET_webhook_after_delete.json",
        StatusCode::NOT_FOUND,
    )
    .await;
    let r = get(&s, "/webhooks").await;
    let list: Value = r.json().await.unwrap();
    assert_eq!(list["_links"]["pb:webhooks"].as_array().unwrap().len(), 3);
}

/// The receiver's address, the Ruby broker's user agent and request sizes differ.
fn normalise_execution(v: Value, receiver_base: &str) -> Value {
    let text = serde_json::to_string(&v).unwrap();
    let text = text
        .replace(
            &receiver_base.trim_start_matches("http://").to_owned(),
            "RECEIVER",
        )
        .replace("host.docker.internal:18100", "RECEIVER")
        .replace("host.docker.internal:18101", "RECEIVER")
        .replace("Pact Broker v2.121.2", "UA");
    let text = regex::Regex::new(r"Pactling/[0-9.]+")
        .unwrap()
        .replace_all(&text, "UA")
        .into_owned();
    let text = regex::Regex::new(r#"user-agent: [^\\]*"#)
        .unwrap()
        .replace_all(&text, "user-agent: UA");
    let text = regex::Regex::new(r#"content-length(\\?": ?\\?"|: )\d+"#)
        .unwrap()
        .replace_all(&text, "content-length${1}L");
    let text = regex::Regex::new(r"verification-results/\d+")
        .unwrap()
        .replace_all(&text, "verification-results/N");
    normalise(serde_json::from_str(&text).unwrap())
}

async fn assert_execution_matches(
    s: &common::TestServer,
    receiver_base: &str,
    r: reqwest::Response,
    name: &str,
    status: StatusCode,
) -> Value {
    assert_eq!(r.status(), status, "{name}");
    let got: Value = r.json().await.unwrap();
    let left = normalise_execution(stable(got.clone()), receiver_base);
    let right = normalise_execution(
        stable(fixture(&format!("phase5/{name}"), &s.base_url)),
        receiver_base,
    );
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

/// The n-th request the Ruby broker sent to its receiver during the capture.
fn captured_request(n: usize) -> Value {
    let path = format!(
        "{}/tests/fixtures/cli/phase5/receiver.jsonl",
        env!("CARGO_MANIFEST_DIR")
    );
    let raw = std::fs::read_to_string(path).unwrap();
    serde_json::from_str(raw.lines().nth(n).unwrap()).unwrap()
}

/// Compares a received request with a captured one: method, path, the headers the broker
/// chose (not host, length or user agent) and the body with hosts normalised.
fn assert_received_like(got: &common::Received, want: &Value, s: &common::TestServer) {
    assert_eq!(got.method, want["method"].as_str().unwrap());
    assert_eq!(got.path, want["path"].as_str().unwrap());
    let skip = [
        "host",
        "content-length",
        "user-agent",
        "connection",
        "accept-encoding",
    ];
    let mut got_headers: Vec<(String, String)> = got
        .headers
        .iter()
        .filter(|(k, _)| !skip.contains(&k.as_str()))
        .cloned()
        .collect();
    let mut want_headers: Vec<(String, String)> = want["headers"]
        .as_object()
        .unwrap()
        .iter()
        .filter(|(k, _)| !skip.contains(&k.as_str()))
        .map(|(k, v)| (k.clone(), v.as_str().unwrap().to_owned()))
        .collect();
    got_headers.sort();
    want_headers.sort();
    assert_eq!(got_headers, want_headers, "{}", got.path);
    let hosts = regex::Regex::new(r"http://(127\.0\.0\.1|localhost):\d+").unwrap();
    let got_body = hosts
        .replace_all(&got.body.replace(&s.base_url, "http://HOST"), "http://HOST")
        .into_owned();
    let want_body = regex::Regex::new(r"verification-results/\d+")
        .unwrap()
        .replace_all(
            &hosts.replace_all(want["body"].as_str().unwrap(), "http://HOST"),
            "verification-results/N",
        )
        .into_owned();
    let got_body = regex::Regex::new(r"verification-results/\d+")
        .unwrap()
        .replace_all(&got_body, "verification-results/N")
        .into_owned();
    assert_eq!(got_body, want_body, "{}", got.path);
}

#[tokio::test]
async fn test_execution_matches_the_ruby_broker() {
    let s = common::start_with(http_config()).await;
    let receiver = common::receiver().await;
    seed(&s).await;
    let body_template: Value =
        cli_exchange("create-webhook-consumer-provider", "POST").body["request"]["body"].clone();
    let r = post(&s, "/webhooks/provider/Provider/consumer/Consumer", &json!({
        "description": "Trigger Provider build", "events": [{"name": "contract_content_changed"}],
        "request": {"method": "POST", "url": format!("{}/hook/${{pactbroker.consumerName}}", receiver.base_url),
                    "headers": {"Content-Type": "application/json", "Authorization": "Bearer hook-secret"}, "body": body_template}
    })).await;
    assert_eq!(r.status(), StatusCode::CREATED);
    let cp: Value = r.json().await.unwrap();
    let r = post(&s, "/webhooks", &json!({"request":{"method":"POST","url":format!("{}/hook/global", receiver.base_url),"username":"hookuser","password":"hookpass","headers":{"X-Token":"abc","Accept":"application/json"},"body":{"template":"${pactbroker.consumerVersionNumber}"}}})).await;
    let global: Value = r.json().await.unwrap();

    let r = post(
        &s,
        &format!("/webhooks/{}/execute", cp["uuid"].as_str().unwrap()),
        &json!({}),
    )
    .await;
    assert_eq!(r.headers()["content-type"], HAL);
    assert_execution_matches(
        &s,
        &receiver.base_url,
        r,
        "POST_webhook_execute.json",
        StatusCode::OK,
    )
    .await;
    let r = post(
        &s,
        &format!("/webhooks/{}/execute", global["uuid"].as_str().unwrap()),
        &json!({}),
    )
    .await;
    assert_execution_matches(
        &s,
        &receiver.base_url,
        r,
        "POST_webhook_execute_global.json",
        StatusCode::OK,
    )
    .await;
    let r = post(&s, "/webhooks/execute", &json!({"consumer":{"name":"Consumer"},"provider":{"name":"Provider"},"request":{"method":"POST","url":format!("{}/hook/unsaved", receiver.base_url),"body":serde_json::to_string(&body_template).unwrap()}})).await;
    assert_execution_matches(
        &s,
        &receiver.base_url,
        r,
        "POST_webhooks_execute_unsaved.json",
        StatusCode::OK,
    )
    .await;
    let r = post(&s, "/webhooks/nope/execute", &json!({})).await;
    assert_matches(
        &s,
        r,
        "POST_webhooks_execute_unknown.json",
        StatusCode::NOT_FOUND,
    )
    .await;
    let failing_receiver = common::receiver_answering(500).await;
    let r = post(
        &s,
        "/webhooks/execute",
        &json!({"request":{"method":"POST","url":format!("{}/hook", failing_receiver.base_url)}}),
    )
    .await;
    let failing = assert_execution_matches(
        &s,
        &failing_receiver.base_url,
        r,
        "POST_webhook_execute_failing.json",
        StatusCode::OK,
    )
    .await;
    assert_eq!(failing["success"], false);

    // What the receiver got is what the Ruby broker's receiver got (capture lines 1 to 3
    // are the saved webhook, the global one and the unsaved one).
    let got = receiver.wait_for(3, 5).await;
    assert_eq!(got.len(), 3);
    assert_received_like(&got[0], &captured_request(1), &s);
    assert_received_like(&got[1], &captured_request(2), &s);
    assert_received_like(&got[2], &captured_request(3), &s);
    let failed = failing_receiver.wait_for(1, 5).await;
    assert_eq!(failed[0].path, "/hook");
    assert_eq!(
        failed[0].body, "null",
        "no body is sent as the string null, as Net::HTTP does"
    );
}
