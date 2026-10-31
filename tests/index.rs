mod common;

#[tokio::test]
async fn index_matches_ruby_broker_fixture() {
    let s = common::start().await;
    let res = s.client.get(&s.base_url).send().await.unwrap();
    assert_eq!(res.status(), 200);
    assert_eq!(
        res.headers()["content-type"],
        "application/hal+json;charset=utf-8"
    );
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(body, common::fixture("GET_index.json", &s.base_url));
}

#[tokio::test]
async fn every_response_carries_broker_headers() {
    let s = common::start().await;
    let res = s.client.get(&s.base_url).send().await.unwrap();
    assert_eq!(res.headers()["x-content-type-options"], "nosniff");
    assert!(res.headers().contains_key("x-pact-broker-version"));
}

#[tokio::test]
async fn head_on_index_is_405_with_allow() {
    let s = common::start().await;
    let res = s.client.head(&s.base_url).send().await.unwrap();
    assert_eq!(res.status(), 405);
    assert_eq!(res.headers()["allow"], "GET, OPTIONS");
}
