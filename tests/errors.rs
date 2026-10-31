mod common;

#[tokio::test]
async fn unknown_route_is_plain_json_404() {
    let s = common::start().await;
    let res = s
        .client
        .get(format!("{}/environments/nope", s.base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 404);
    assert_eq!(
        res.headers()["content-type"],
        "application/json;charset=utf-8"
    );
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(
        body,
        common::fixture("GET_environments_nope.json", &s.base_url)
    );
}

#[tokio::test]
async fn unknown_pacticipant_is_hal_404_with_message() {
    let s = common::start().await;
    let res = s
        .client
        .get(format!("{}/pacticipants/Nope", s.base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 404);
    assert_eq!(
        res.headers()["content-type"],
        "application/hal+json;charset=utf-8"
    );
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(
        body,
        common::fixture("GET_pacticipants_Nope.json", &s.base_url)
    );
}
