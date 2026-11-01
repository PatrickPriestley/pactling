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
