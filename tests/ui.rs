mod common;

use common::{TestServer, start};
use serde_json::json;

async fn publish(s: &TestServer, consumer: &str, provider: &str, version: &str) {
    let response = s
        .client
        .put(format!(
            "{}/pacts/provider/{provider}/consumer/{consumer}/version/{version}",
            s.base_url
        ))
        .json(&json!({"consumer":{"name":consumer},"provider":{"name":provider},"interactions":[]}))
        .send()
        .await
        .unwrap();
    assert!(response.status().is_success());
}

#[tokio::test]
async fn ui_index_lists_integrations() {
    let s = start().await;
    publish(&s, "Checkout", "Payments", "1").await;
    publish(&s, "Checkout", "Payments", "2").await;
    publish(&s, "Orders", "Inventory", "1").await;
    let response = s
        .client
        .get(format!("{}/ui/", s.base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(
        response.headers()["content-type"],
        "text/html; charset=utf-8"
    );
    assert_eq!(response.headers()["cache-control"], "no-store");
    let body = response.text().await.unwrap();
    for content in [
        "<h1>Integrations</h1>",
        "Checkout",
        "Payments",
        "Orders",
        "Inventory",
        "href=\"/ui/pacts/provider/Payments/consumer/Checkout/version/2\"",
        "href=\"/ui/matrix/provider/Payments/consumer/Checkout\"",
        "<th scope=\"col\">Consumer</th>",
    ] {
        assert!(body.contains(content), "missing {content}: {body}");
    }
    assert!(!body.contains("/Checkout/version/1\""));
    assert!(!body.contains("<script"));
    assert!(body.contains("href=\"/ui/style.css\""));
}

#[tokio::test]
async fn ui_routes_preserve_api_negotiation() {
    let s = start().await;
    publish(&s, "Checkout", "Payments", "1").await;
    let paths = [
        "/",
        "/integrations",
        "/pacts/provider/Payments/consumer/Checkout/version/1",
        "/matrix/provider/Payments/consumer/Checkout",
    ];
    let accepts = [
        "application/hal+json",
        "application/json",
        "text/html",
        "*/*",
        "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
        "application/json;q=1,text/html;q=0.5",
        "text/html;q=0,application/hal+json;q=1",
    ];
    let mut before = Vec::new();
    for path in paths {
        for accept in accepts {
            let r = s
                .client
                .get(format!("{}{path}", s.base_url))
                .header("Accept", accept)
                .send()
                .await
                .unwrap();
            before.push((
                r.status(),
                r.headers()["content-type"].clone(),
                r.bytes().await.unwrap(),
            ));
        }
    }
    let ui = s
        .client
        .get(format!("{}/ui/", s.base_url))
        .header("Accept", "text/html")
        .send()
        .await
        .unwrap();
    assert_eq!(ui.status(), 200);
    assert_eq!(ui.headers()["content-type"], "text/html; charset=utf-8");
    let mut i = 0;
    for path in paths {
        for accept in accepts {
            let r = s
                .client
                .get(format!("{}{path}", s.base_url))
                .header("Accept", accept)
                .send()
                .await
                .unwrap();
            assert_eq!(
                (
                    r.status(),
                    r.headers()["content-type"].clone(),
                    r.bytes().await.unwrap()
                ),
                before[i],
                "{path} {accept}"
            );
            i += 1;
        }
    }
}

#[tokio::test]
async fn ui_index_empty_and_methods() {
    let s = start().await;
    let body = s
        .client
        .get(format!("{}/ui/", s.base_url))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(body.contains("No integrations have been published."));
    for path in ["/ui/", "/ui/style.css"] {
        let r = s
            .client
            .post(format!("{}{path}", s.base_url))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 405);
        assert_eq!(r.headers()["allow"], "GET, OPTIONS");
        let r = s
            .client
            .request(reqwest::Method::OPTIONS, format!("{}{path}", s.base_url))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 200);
        assert_eq!(r.headers()["access-control-allow-methods"], "GET, OPTIONS");
    }
    let r = s
        .client
        .get(format!("{}/ui/style.css", s.base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    assert_eq!(r.headers()["content-type"], "text/css; charset=utf-8");
    assert!(r.text().await.unwrap().contains("overflow-wrap"));
}

#[tokio::test]
async fn ui_index_database_error_is_safe() {
    let path = std::env::temp_dir().join(format!("pactling-ui-error-{}.db", uuid::Uuid::new_v4()));
    let url = format!("sqlite://{}?mode=rwc", path.display());
    let mut config = pactling::config::Config::in_memory();
    config.database_url = url.clone();
    let s = common::start_with(config).await;
    let pool = sqlx::SqlitePool::connect(&url).await.unwrap();
    sqlx::query("DROP TABLE pact_publications")
        .execute(&pool)
        .await
        .unwrap();
    let r = s
        .client
        .get(format!("{}/ui/", s.base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 500);
    assert_eq!(r.headers()["content-type"], "text/html; charset=utf-8");
    let body = r.text().await.unwrap();
    assert!(body.contains("Unable to load integrations."));
    assert!(!body.contains("pact_publications"));
    pool.close().await;
    std::fs::remove_file(path).unwrap();
}

async fn publish_document(
    s: &TestServer,
    c: &str,
    p: &str,
    v: &str,
    interactions: serde_json::Value,
) {
    let encode = pactling::http::hal::encode;
    let r = s
        .client
        .put(format!(
            "{}/pacts/provider/{}/consumer/{}/version/{}",
            s.base_url,
            encode(p),
            encode(c),
            encode(v)
        ))
        .json(&json!({"consumer":{"name":c},"provider":{"name":p},"interactions":interactions}))
        .send()
        .await
        .unwrap();
    assert!(r.status().is_success(), "{}", r.text().await.unwrap());
}

#[tokio::test]
async fn ui_pact_renders_exact_version() {
    let s = start().await;
    publish_document(&s,"Checkout","Payments","1",json!([
        {"description":"old contract","providerState":"order exists","request":{"method":"GET","path":"/old"},"response":{"status":200}},
        {"description":"modern states","providerStates":[{"name":"ready","params":{"id":42}}],"request":{"method":"POST","path":"/new"},"response":{"status":201}}
    ])).await;
    publish_document(&s,"Checkout","Payments","2",json!([{ "description":"new contract", "request":{"method":"GET","path":"/new"},"response":{"status":202}}])).await;
    let r = s
        .client
        .get(format!(
            "{}/ui/pacts/provider/Payments/consumer/Checkout/version/1",
            s.base_url
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    assert_eq!(r.headers()["content-type"], "text/html; charset=utf-8");
    assert_eq!(r.headers()["cache-control"], "no-store");
    let body = r.text().await.unwrap();
    for text in [
        "Checkout",
        "Payments",
        "Consumer version",
        "old contract",
        "order exists",
        "modern states",
        "ready",
        "42",
        "/old",
        "200",
        "Request",
        "Response",
        "Published",
        "API document",
        "/ui/matrix/provider/Payments/consumer/Checkout",
    ] {
        assert!(body.contains(text), "missing {text}: {body}");
    }
    assert!(!body.contains("new contract"));
}

#[tokio::test]
async fn ui_escapes_untrusted_content() {
    let s = start().await;
    publish_document(&s,"C","P","1",json!([{"description":"<script>alert(1)</script>","providerState":"<img src=x onerror=bad>","request":{"method":"GET","path":"/<svg onload=bad>"},"response":{"status":200,"body":"<script>bad</script>"}}])).await;
    let body = s
        .client
        .get(format!(
            "{}/ui/pacts/provider/P/consumer/C/version/1",
            s.base_url
        ))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(body.contains("&lt;script&gt;alert(1)&lt;/script&gt;"));
    assert!(body.contains("&lt;img src=x onerror=bad&gt;"));
    assert!(body.contains("&lt;svg onload=bad&gt;"));
    assert!(!body.contains("<script"));
    assert!(!body.contains("<img"));
    assert!(!body.contains("<svg"));
}

#[tokio::test]
async fn ui_pact_handles_empty_missing_and_methods() {
    let s = start().await;
    publish(&s, "C", "P", "1").await;
    let path = format!("{}/ui/pacts/provider/P/consumer/C/version/1", s.base_url);
    let body = s
        .client
        .get(&path)
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(body.contains("No interactions are recorded."));
    let missing = s
        .client
        .get(format!(
            "{}/ui/pacts/provider/P/consumer/C/version/missing",
            s.base_url
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(missing.status(), 404);
    assert_eq!(
        missing.headers()["content-type"],
        "text/html; charset=utf-8"
    );
    assert!(missing.text().await.unwrap().contains("Pact not found"));
    let r = s.client.post(&path).send().await.unwrap();
    assert_eq!(r.status(), 405);
    assert_eq!(r.headers()["allow"], "GET, OPTIONS");
    let r = s
        .client
        .request(reqwest::Method::OPTIONS, &path)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    assert_eq!(r.headers()["access-control-allow-methods"], "GET, OPTIONS");
}

#[tokio::test]
async fn ui_encodes_links_and_rejects_unsafe_schemes() {
    let s = start().await;
    let (c, p, v) = ("C &<>/雪", "P ?#\"", "v/1");
    publish_document(&s, c, p, v, json!([])).await;
    let encode = pactling::http::hal::encode;
    let route = format!(
        "/ui/pacts/provider/{}/consumer/{}/version/{}",
        encode(p),
        encode(c),
        encode(v)
    );
    let r = s
        .client
        .get(format!("{}{route}", s.base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let body = r.text().await.unwrap();
    assert!(body.contains("C &amp;&lt;&gt;/雪"));
    assert!(body.contains(&format!(
        "href=\"/ui/matrix/provider/{}/consumer/{}\"",
        encode(p),
        encode(c)
    )));
    assert!(body.contains(&format!(
        "href=\"/pacts/provider/{}/consumer/{}/version/{}\"",
        encode(p),
        encode(c),
        encode(v)
    )));
}

#[tokio::test]
async fn ui_pact_database_error_is_safe() {
    let path = std::env::temp_dir().join(format!(
        "pactling-ui-pact-error-{}.db",
        uuid::Uuid::new_v4()
    ));
    let mut config = pactling::config::Config::in_memory();
    config.database_url = format!("sqlite://{}?mode=rwc", path.display());
    let url = config.database_url.clone();
    let s = common::start_with(config).await;
    let pool = sqlx::SqlitePool::connect(&url).await.unwrap();
    sqlx::query("DROP TABLE pact_publications")
        .execute(&pool)
        .await
        .unwrap();
    let r = s
        .client
        .get(format!(
            "{}/ui/pacts/provider/P/consumer/C/version/1",
            s.base_url
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 500);
    let body = r.text().await.unwrap();
    assert!(body.contains("Unable to load pact"));
    assert!(!body.contains("pact_publications"));
    pool.close().await;
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn ui_pact_optional_interactions_and_empty_notice() {
    let s = start().await;
    publish_document(
        &s,
        "C",
        "P",
        "1",
        json!([{"description":"one","request":{},"response":{}}]),
    )
    .await;
    let body = s
        .client
        .get(format!(
            "{}/ui/pacts/provider/P/consumer/C/version/1",
            s.base_url
        ))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(!body.contains("No interactions are recorded."));
    publish_document(&s, "C", "P", "2", json!(null)).await;
    let body = s
        .client
        .get(format!(
            "{}/ui/pacts/provider/P/consumer/C/version/2",
            s.base_url
        ))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(body.contains("No interactions are recorded."));
    assert!(body.contains("Full contract"));
}

async fn matrix_seed(s: &TestServer) {
    for (version, success) in [("1", Some(true)), ("2", Some(false)), ("3", None)] {
        let r: serde_json::Value = s.client.put(format!("{}/pacts/provider/Payments/consumer/Checkout/version/{version}",s.base_url))
            .json(&json!({"consumer":{"name":"Checkout"},"provider":{"name":"Payments"},"interactions":[{"description":version,"request":{"method":"GET","path":"/"},"response":{"status":200}}]}))
            .send().await.unwrap().json().await.unwrap();
        if let Some(success) = success {
            let result = s
                .client
                .post(
                    r["_links"]["pb:publish-verification-results"]["href"]
                        .as_str()
                        .unwrap(),
                )
                .json(&json!({"providerApplicationVersion":"1","success":success}))
                .send()
                .await
                .unwrap();
            assert!(result.status().is_success());
        }
        let r = s
            .client
            .put(format!(
                "{}/pacticipants/Checkout/branches/main/versions/{version}",
                s.base_url
            ))
            .json(&json!({}))
            .send()
            .await
            .unwrap();
        assert!(r.status().is_success());
    }
}

async fn matrix_html(s: &TestServer, query: &str) -> (u16, String) {
    let r = s
        .client
        .get(format!(
            "{}/ui/matrix/provider/Payments/consumer/Checkout?{query}",
            s.base_url
        ))
        .send()
        .await
        .unwrap();
    let status = r.status().as_u16();
    (status, r.text().await.unwrap())
}

#[tokio::test]
async fn ui_matrix_preserves_rows_and_nulls() {
    let s = start().await;
    matrix_seed(&s).await;
    let (status, body) = matrix_html(&s, "").await;
    assert_eq!(status, 200);
    assert!(body.contains("<h1>Matrix</h1>"));
    assert!(body.contains("Unknown"));
    assert!(body.contains("Failed"));
    assert!(body.contains("Successful"));
    let api: serde_json::Value = s
        .client
        .get(format!(
            "{}/matrix/provider/Payments/consumer/Checkout",
            s.base_url
        ))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let expected: Vec<_> = api["matrix"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            format!(
                "data-consumer-version=\"{}\" data-provider-version=\"{}\"",
                r["consumer"]["version"]["number"].as_str().unwrap_or(""),
                r["provider"]["version"]["number"].as_str().unwrap_or("")
            )
        })
        .collect();
    let mut offset = 0;
    for row in expected {
        let found = body[offset..]
            .find(&row)
            .unwrap_or_else(|| panic!("missing row {row}: {body}"));
        offset += found + row.len();
    }
    assert_eq!(
        body.matches("data-consumer-version=").count(),
        api["matrix"].as_array().unwrap().len()
    );
    let (_, ignored) = matrix_html(&s, "ignore[][pacticipant]=Payments").await;
    assert!(ignored.contains("Ignored"));
    assert!(body.contains("name=\"ui_form\" value=\"matrix\""));
}

#[tokio::test]
async fn ui_matrix_filters_match_api() {
    let s = start().await;
    matrix_seed(&s).await;
    for (mode, value, expected) in [
        ("version", "1", "1"),
        ("latest", "", "3"),
        ("branch", "main", "3"),
        ("main", "", "3"),
    ] {
        let q = format!(
            "ui_form=matrix&consumer_mode={mode}&consumer_value={value}&provider_mode=all&provider_value=&latestby=cvpv"
        );
        let (status, body) = matrix_html(&s, &q).await;
        assert_eq!(status, 200, "{q}: {body}");
        assert!(
            body.contains(&format!("data-consumer-version=\"{expected}\"")),
            "{q}: {body}"
        );
        assert_eq!(body.matches("data-consumer-version=").count(), 1, "{q}");
    }
    let r = s
        .client
        .put(format!(
            "{}/pacticipants/Checkout/versions/1/tags/release",
            s.base_url
        ))
        .json(&json!({}))
        .send()
        .await
        .unwrap();
    assert!(r.status().is_success());
    let(status,body)=matrix_html(&s,"ui_form=matrix&consumer_mode=tag&consumer_value=release&provider_mode=version&provider_value=1&latestby=none").await;
    assert_eq!(status, 200);
    assert!(body.contains("data-consumer-version=\"1\""));
    for group in ["cp", "cvp", "cvpv"] {
        let q = format!("q[][pacticipant]=Checkout&q[][pacticipant]=Payments&latestby={group}");
        let api: serde_json::Value = s
            .client
            .get(format!("{}/matrix?{q}", s.base_url))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let (status, body) = matrix_html(&s, &q).await;
        assert_eq!(status, 200);
        assert_eq!(
            body.matches("data-consumer-version=").count(),
            api["matrix"].as_array().unwrap().len()
        );
    }
    let (status, body) = matrix_html(
        &s,
        "q[][pacticipant]=Checkout&q[][version]=&q[][mainBranch]=true",
    )
    .await;
    assert_eq!(status, 200);
    let api: serde_json::Value = s.client.get(format!("{}/matrix?q[][pacticipant]=Checkout&q[][version]=&q[][mainBranch]=true&q[][pacticipant]=Payments",s.base_url)).send().await.unwrap().json().await.unwrap();
    assert_eq!(
        body.matches("data-consumer-version=").count(),
        api["matrix"].as_array().unwrap().len(),
        "canonical empty version must not be silently stripped"
    );
}

#[tokio::test]
async fn ui_matrix_validates_pair_and_form_and_methods() {
    let s = start().await;
    matrix_seed(&s).await;
    for q in [
        "q[][pacticipant]=Other",
        "q[][pacticipant]=Checkout&q[][pacticipant]=Checkout",
        "ui_form=matrix&consumer_mode=bad",
        "ui_form=matrix&consumer_mode=version&consumer_value=",
        "ui_form=matrix&consumer_mode=all&provider_mode=all&latestby=bad",
        "environment=missing",
    ] {
        let (status, body) = matrix_html(&s, q).await;
        assert_eq!(status, 400, "{q}: {body}");
        assert!(body.contains("<html"));
    }
    for (method, status) in [
        (reqwest::Method::POST, 405),
        (reqwest::Method::OPTIONS, 200),
    ] {
        let r = s
            .client
            .request(
                method,
                format!(
                    "{}/ui/matrix/provider/Payments/consumer/Checkout",
                    s.base_url
                ),
            )
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), status);
    }
    let r = s
        .client
        .get(format!(
            "{}/ui/matrix/provider/Missing/consumer/Missing",
            s.base_url
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 400);
    assert!(r.text().await.unwrap().contains("Unable to load matrix"));
}

#[tokio::test]
async fn ui_matrix_escapes_names_and_database_error() {
    let s = start().await;
    let provider = "Pay <&";
    let consumer = "Check <&";
    let enc = pactling::http::hal::encode;
    let r = s
        .client
        .put(format!(
            "{}/pacts/provider/{}/consumer/{}/version/1",
            s.base_url,
            enc(provider),
            enc(consumer)
        ))
        .json(&json!({"consumer":{"name":consumer},"provider":{"name":provider},"interactions":[]}))
        .send()
        .await
        .unwrap();
    assert!(r.status().is_success());
    let r = s
        .client
        .get(format!(
            "{}/ui/matrix/provider/{}/consumer/{}",
            s.base_url,
            enc(provider),
            enc(consumer)
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let body = r.text().await.unwrap();
    assert!(body.contains("Pay &lt;&amp;"));
    assert!(!body.contains("Pay <&"));
    assert!(body.contains("Check &lt;&amp;"));
    assert!(!body.contains("Check <&"));
    assert!(body.contains("action=\"/ui/matrix/provider/Pay%20%3C%26/consumer/Check%20%3C%26\""));
    assert!(body.contains("Pay%20%3C%26"));
    let path = std::env::temp_dir().join(format!("ui-matrix-{}.db", uuid::Uuid::new_v4()));
    let url = format!("sqlite://{}?mode=rwc", path.display());
    let mut config = pactling::config::Config::in_memory();
    config.database_url = url.clone();
    let s = common::start_with(config).await;
    let pool = sqlx::SqlitePool::connect(&url).await.unwrap();
    sqlx::query("DROP TABLE pacticipants")
        .execute(&pool)
        .await
        .unwrap();
    let (status, body) = matrix_html(&s, "").await;
    assert_eq!(status, 500);
    assert!(body.contains("Unable to load matrix"));
    assert!(!body.contains("no such table"));
}

#[tokio::test]
async fn ui_matrix_reports_empty_and_errors() {
    let s = start().await;
    matrix_seed(&s).await;
    let (status, body) = matrix_html(&s, "q[][pacticipant]=Checkout&q[][version]=absent").await;
    assert_eq!(status, 200);
    assert!(body.contains("No matrix rows match these selectors."));
    let (_, body) = matrix_html(&s, "q[][pacticipant]=Other").await;
    assert!(body.contains("Selectors must belong to this consumer/provider pair."));
    let (_, body) = matrix_html(&s, "environment=missing").await;
    assert!(body.contains("Environment with name"));
    assert!(body.contains("missing"));
}

#[test]
fn ui_matrix_form_query_mapping() {
    use pactling::http::ui::query::{encode_fields, field, fields, matrix_query, selector_fields};
    let repeated = fields("x=a%26b+c&x=%3Ctag%3E&empty=");
    assert_eq!(
        repeated,
        vec![
            ("x".into(), "a&b c".into()),
            ("x".into(), "<tag>".into()),
            ("empty".into(), "".into())
        ]
    );
    assert_eq!(field(&repeated, "x"), "a&b c");
    assert_eq!(field(&repeated, "absent"), "");
    assert_eq!(fields(&encode_fields(&repeated)), repeated);
    for (mode, value, expected) in [
        ("all", "", vec![]),
        ("latest", "unused", vec![("q[][latest]", "true")]),
        (
            "main",
            "unused",
            vec![("q[][latest]", "true"), ("q[][mainBranch]", "true")],
        ),
        ("version", "<&+", vec![("q[][version]", "<&+")]),
        (
            "branch",
            "main",
            vec![("q[][latest]", "true"), ("q[][branch]", "main")],
        ),
        (
            "tag",
            "release",
            vec![("q[][latest]", "true"), ("q[][tag]", "release")],
        ),
    ] {
        let actual = selector_fields("A &<+", mode, value).unwrap();
        let expected: Vec<(String, String)> = std::iter::once(("q[][pacticipant]", "A &<+"))
            .chain(expected)
            .map(|(k, v)| (k.into(), v.into()))
            .collect();
        assert_eq!(actual, expected);
        assert_eq!(fields(&encode_fields(&actual)), actual);
    }
    for mode in ["version", "branch", "tag", "invalid", ""] {
        assert!(selector_fields("C", mode, "").is_err());
    }
    for grouping in ["none", "cvp", "cp", "cvpv"] {
        let (q, raw) = matrix_query(
            &format!("ui_form=matrix&consumer_mode=all&provider_mode=all&latestby={grouping}"),
            "C",
            "P",
        )
        .unwrap();
        assert_eq!(
            q.latestby.as_deref(),
            if grouping == "none" {
                None
            } else {
                Some(grouping)
            }
        );
        assert!(raw.contains("q%5B%5D%5Bpacticipant%5D=C"));
        assert_eq!(
            q.selectors
                .iter()
                .map(|s| s.pacticipant.as_str())
                .collect::<Vec<_>>(),
            vec!["C", "P"]
        );
    }
    let raw = "q[][pacticipant]=P&q[][version]=2&ignore[][pacticipant]=C&ignore[][version]=1&ignore[][pacticipant]=P";
    let (q, unchanged) = matrix_query(raw, "C", "P").unwrap();
    assert_eq!(unchanged, raw);
    assert_eq!(
        q.selectors
            .iter()
            .find(|s| s.pacticipant == "P")
            .unwrap()
            .version
            .as_deref(),
        Some("2")
    );
    assert_eq!(
        q.ignore,
        vec![("C".into(), Some("1".into())), ("P".into(), None)]
    );
}

#[tokio::test]
async fn ui_matrix_self_integration_keeps_positional_selectors() {
    let s = start().await;
    publish(&s, "Self", "Self", "1").await;
    let canonical = "q[][pacticipant]=Self&q[][version]=1&q[][pacticipant]=Self&q[][latest]=true";
    let api = s
        .client
        .get(format!("{}/matrix?{canonical}", s.base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(api.status(), 200);
    let expected: serde_json::Value = api.json().await.unwrap();
    let r=s.client.get(format!("{}/ui/matrix/provider/Self/consumer/Self?ui_form=matrix&consumer_mode=version&consumer_value=1&provider_mode=latest&latestby=none",s.base_url)).send().await.unwrap();
    assert_eq!(r.status(), 200);
    let body = r.text().await.unwrap();
    assert_eq!(
        body.matches("data-consumer-version=").count(),
        expected["matrix"].as_array().unwrap().len()
    );
    let (q, _) = pactling::http::ui::query::matrix_query(canonical, "Self", "Self").unwrap();
    assert_eq!(q.selectors[0].version.as_deref(), Some("1"));
    assert!(q.selectors[1].latest);
    assert!(q.selectors[1].version.is_none());
}

#[tokio::test]
async fn ui_matrix_nonempty_considered_and_safe_errors() {
    let s = start().await;
    matrix_seed(&s).await;
    let (_, body) = matrix_html(&s, "").await;
    assert!(!body.contains("No matrix rows match these selectors."));
    assert!(body.contains("Considered"));
    assert!(!body.contains(">Ignored<"));
    let (status, body) = matrix_html(&s, "environment=%3Cscript%3E").await;
    assert_eq!(status, 400);
    assert!(body.contains("&lt;script&gt;"));
    assert!(!body.contains("<script>"));
}

#[test]
fn ui_matrix_canonical_selector_order_is_preserved() {
    use pactling::{http::ui::query::matrix_query, service::matrix::MatrixQuery};
    let raw =
        "q[][pacticipant]=P&q[][version]=2&q[][pacticipant]=C&q[][mainBranch]=true&latestby=cp";
    let (q, unchanged) = matrix_query(raw, "C", "P").unwrap();
    assert_eq!(q, MatrixQuery::parse(Some(raw)));
    assert_eq!(unchanged, raw);
    let (q, _) = matrix_query("q[][pacticipant]=P&q[][version]=2", "C", "P").unwrap();
    assert_eq!(q.selectors[0].pacticipant, "P");
    assert_eq!(q.selectors[1].pacticipant, "C");
}

async fn explanation_seed(s: &TestServer) {
    matrix_seed(s).await;
    let env: serde_json::Value = s
        .client
        .post(format!("{}/environments", s.base_url))
        .json(&json!({"name":"test","production":false}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let r = s
        .client
        .post(format!(
            "{}/pacticipants/Payments/versions/1/deployed-versions/environment/{}",
            s.base_url,
            env["uuid"].as_str().unwrap()
        ))
        .json(&json!({}))
        .send()
        .await
        .unwrap();
    assert!(r.status().is_success());
    for (name, version, tag) in [("Payments", "1", "prod"), ("Checkout", "1", "release")] {
        assert!(
            s.client
                .put(format!(
                    "{}/pacticipants/{name}/versions/{version}/tags/{tag}",
                    s.base_url
                ))
                .json(&json!({}))
                .send()
                .await
                .unwrap()
                .status()
                .is_success()
        );
    }
}
async fn explanation_html(s: &TestServer, query: &str) -> (u16, String) {
    let r = s
        .client
        .get(format!("{}/ui/deployment-explanation?{query}", s.base_url))
        .send()
        .await
        .unwrap();
    (r.status().as_u16(), r.text().await.unwrap())
}
#[tokio::test]
async fn ui_explanation_matches_shared_evaluation() {
    let s = start().await;
    explanation_seed(&s).await;
    for (version, decision) in [("1", "allowed"), ("2", "blocked"), ("3", "unknown")] {
        let q = format!("pacticipant=Checkout&version={version}&environment=test");
        let api: serde_json::Value = s
            .client
            .get(format!(
                "{}/pactling/v1/can-i-deploy/explanation?{q}",
                s.base_url
            ))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(api["decision"], decision);
        let (status, body) = explanation_html(&s, &q).await;
        assert_eq!(status, 200, "{body}");
        assert!(
            body.contains(&format!("data-decision=\"{decision}\"")),
            "{body}"
        );
        for value in [
            "recorded_only",
            "not_checked",
            "Evaluated at",
            "Checkout",
            "Payments",
            "Candidate provenance",
        ] {
            assert!(body.contains(value), "{value}: {body}");
        }
        for reason in api["reasons"].as_array().unwrap() {
            assert!(body.contains(reason["code"].as_str().unwrap()));
        }
        for action in api["nextActions"].as_array().unwrap() {
            assert!(body.contains(action["type"].as_str().unwrap()));
        }
        assert!(body.contains("data-consumer-version=\""));
        assert!(body.contains("/pacts/provider/Payments/consumer/Checkout/"));
        assert!(body.contains(&format!("name=\"selector_value\" value=\"{version}\"")));
    }
    let (_, body) = explanation_html(&s, "pacticipant=Checkout&version=3&environment=test").await;
    assert!(body.contains("data-provider-version=\"\""));
    assert!(body.contains("data-candidate-version=\"1\""));
    let (_, body) = explanation_html(&s, "pacticipant=Checkout&environment=test").await;
    assert!(body.contains("data-decision=\"unknown\""));
    assert!(body.contains("VERIFICATION_FAILED"));
    assert!(body.contains("VERIFICATION_MISSING"));
}
#[tokio::test]
async fn ui_explanation_form_modes_and_ignores() {
    let s = start().await;
    explanation_seed(&s).await;
    for (mode, value, decision) in [
        ("version", "1", "allowed"),
        ("latest", "", "unknown"),
        ("main", "", "unknown"),
        ("branch", "main", "unknown"),
        ("tag", "release", "allowed"),
        ("all", "", "unknown"),
    ] {
        let q = format!(
            "ui_form=explanation&pacticipant=Checkout&selector_mode={mode}&selector_value={value}&target_mode=environment&target_value=test&ignore=&ignore="
        );
        let (status, body) = explanation_html(&s, &q).await;
        assert_eq!(status, 200, "{q}: {body}");
        assert!(
            body.contains(&format!("data-decision=\"{decision}\"")),
            "{q}: {body}"
        );
        assert!(
            body.contains(&format!("option value=\"{mode}\" selected")),
            "{body}"
        );
        assert!(body.contains("name=\"target_value\" value=\"test\""));
    }
    let q = "ui_form=explanation&pacticipant=Checkout&selector_mode=version&selector_value=1&target_mode=tag&target_value=prod&ignore=Payments%3D1&ignore=Absent";
    let (status, body) = explanation_html(&s, q).await;
    assert_eq!(status, 200, "{body}");
    assert!(body.contains("Payments=1"));
    assert!(body.contains("Absent"));
    assert!(body.contains("ignored"));
    let (status, body) = explanation_html(
        &s,
        "q[][pacticipant]=Checkout&q[][mainBranch]=true&environment=test",
    )
    .await;
    assert_eq!(status, 200);
    assert!(body.contains("data-decision=\"unknown\""));
}
#[tokio::test]
async fn ui_explanation_validates_query() {
    let s = start().await;
    explanation_seed(&s).await;
    let (status, body) = explanation_html(&s, "").await;
    assert_eq!(status, 200);
    assert!(body.contains("<form"));
    assert!(!body.contains("data-decision="));
    for q in [
        "pacticipant=Checkout&version=1",
        "environment=test",
        "pacticipant=Checkout&version=1&environment=missing",
        "ui_form=explanation&pacticipant=&selector_mode=version&selector_value=1&target_mode=environment&target_value=test",
        "ui_form=explanation&pacticipant=Checkout&selector_mode=version&selector_value=&target_mode=environment&target_value=test",
        "ui_form=explanation&pacticipant=Checkout&selector_mode=nope&target_mode=environment&target_value=test",
        "ui_form=explanation&pacticipant=Checkout&selector_mode=main&target_mode=nope&target_value=test",
        "ui_form=explanation&pacticipant=Checkout&selector_mode=main&target_mode=environment&target_value=",
        "ui_form=explanation&pacticipant=Checkout&selector_mode=main&target_mode=environment&target_value=test&ignore=%3D1",
    ] {
        let (status, body) = explanation_html(&s, q).await;
        assert_eq!(status, 400, "{q}: {body}");
        assert!(body.contains("<h1>"));
        assert!(body.contains("<form"));
    }
    let(status,body)=explanation_html(&s,"ui_form=explanation&pacticipant=%3Cscript%3E&selector_mode=version&selector_value=&target_mode=environment&target_value=test").await;
    assert_eq!(status, 400);
    assert!(body.contains("&lt;script&gt;"));
    assert!(!body.contains("<script>"));
    for (method, status) in [
        (reqwest::Method::OPTIONS, 200),
        (reqwest::Method::POST, 405),
    ] {
        let r = s
            .client
            .request(method, format!("{}/ui/deployment-explanation", s.base_url))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), status);
    }
}

#[tokio::test]
async fn ui_explanation_open_codes_and_anchor_targets() {
    let s = start().await;
    explanation_seed(&s).await;
    let mut doc: serde_json::Value = s
        .client
        .get(format!(
            "{}/pactling/v1/can-i-deploy/explanation?pacticipant=Checkout&environment=test",
            s.base_url
        ))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    doc["reasons"][0]["code"] = json!("FUTURE_<REASON>");
    doc["nextActions"][0]["type"] = json!("FUTURE_<ACTION>");
    doc["nextActions"][0]["target"]["otherHref"] = json!("javascript:alert(1)");
    doc["resolution"]["selectors"][0]["extra/key~"] = json!([true, 42]);
    let html = pactling::http::ui::explanation::render_document(&doc).into_string();
    assert!(html.contains("FUTURE_&lt;REASON&gt;"));
    assert!(html.contains("FUTURE_&lt;ACTION&gt;"));
    assert!(!html.contains("<REASON>"));
    assert!(!html.contains("href=\"javascript:"));
    assert!(html.contains("true"));
    assert!(html.contains("42"));
    let mut ids = std::collections::HashSet::new();
    for item in html.split(" id=\"").skip(1) {
        let id = item.split('"').next().unwrap();
        assert!(!id.is_empty());
        assert!(ids.insert(id), "duplicate id {id}");
    }
    for reason in doc["reasons"].as_array().unwrap() {
        for pointer in reason["evidenceRefs"].as_array().unwrap() {
            assert!(
                ids.contains(pointer.as_str().unwrap()),
                "missing anchor {pointer}: {html}"
            );
        }
    }
    assert!(ids.contains("/resolution/selectors/0/extra~1key~0/1"));
}
#[tokio::test]
async fn ui_explanation_canonical_form_values_and_query_encoding() {
    let s = start().await;
    explanation_seed(&s).await;
    for (q, mode, value) in [
        (
            "q[][pacticipant]=Checkout&q[][latest]=true&environment=test",
            "latest",
            "",
        ),
        (
            "q[][pacticipant]=Checkout&q[][mainBranch]=true&environment=test",
            "main",
            "",
        ),
        (
            "q[][pacticipant]=Checkout&q[][branch]=main&environment=test",
            "branch",
            "main",
        ),
        (
            "q[][pacticipant]=Checkout&q[][tag]=release&environment=test",
            "tag",
            "release",
        ),
        (
            "pacticipant=Checkout&to=prod&ignore[][pacticipant]=Payments&ignore[][version]=1&ignore[][pacticipant]=Absent",
            "all",
            "",
        ),
    ] {
        let (status, body) = explanation_html(&s, q).await;
        assert_eq!(status, 200, "{body}");
        assert!(
            body.contains(&format!("option value=\"{mode}\" selected")),
            "{body}"
        );
        assert!(
            body.contains(&format!("name=\"selector_value\" value=\"{value}\"")),
            "{body}"
        );
        if q.contains("to=prod") {
            assert!(body.contains("option value=\"tag\" selected"));
            assert!(body.contains("name=\"target_value\" value=\"prod\""));
            assert!(body.contains("Payments=1"));
            assert!(body.contains("Absent"));
        }
    }
    let (raw, expected) = (
        "ui_form=explanation&pacticipant=A%26B&selector_mode=version&selector_value=v%2F%3D%2B&target_mode=tag&target_value=release%26x&ignore=P%2FQ%3Dv%3D1&ignore=R%26S",
        ("A&B", "v/=+", "release&x"),
    );
    let (q, canonical) = pactling::http::ui::query::explanation_query(raw).unwrap();
    assert_eq!(q.selectors[0].pacticipant, expected.0);
    assert_eq!(q.selectors[0].version.as_deref(), Some(expected.1));
    assert_eq!(q.tag.as_deref(), Some(expected.2));
    assert_eq!(
        q.ignore,
        vec![("P/Q".into(), Some("v=1".into())), ("R&S".into(), None)]
    );
    assert_eq!(
        q,
        pactling::service::matrix::MatrixQuery::parse(Some(&canonical))
    );
    let raw = "q[][pacticipant]=Checkout&q[][version]=&q[][mainBranch]=true&environment=test";
    let (q, canonical) = pactling::http::ui::query::explanation_query(raw).unwrap();
    assert_eq!(canonical, raw);
    assert_eq!(q.selectors[0].version.as_deref(), Some(""));
}
#[tokio::test]
async fn ui_explanation_database_error_is_safe() {
    let path = std::env::temp_dir().join(format!(
        "pactling-ui-explanation-{}.db",
        uuid::Uuid::new_v4()
    ));
    let url = format!("sqlite://{}?mode=rwc", path.display());
    let mut c = pactling::config::Config::in_memory();
    c.database_url = url.clone();
    let s = common::start_with(c).await;
    explanation_seed(&s).await;
    let pool = sqlx::SqlitePool::connect(&url).await.unwrap();
    sqlx::query("DROP TABLE pact_publications")
        .execute(&pool)
        .await
        .unwrap();
    let (status, body) =
        explanation_html(&s, "pacticipant=Checkout&version=1&environment=test").await;
    assert_eq!(status, 500);
    assert!(body.contains("Unable to load explanation."));
    assert!(!body.contains("pact_publications"));
    assert!(body.contains("<form"));
    pool.close().await;
}
