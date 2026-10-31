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
