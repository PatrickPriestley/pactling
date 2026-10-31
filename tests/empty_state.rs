//! Every collection on an empty broker renders exactly what the Ruby broker renders.
mod common;

async fn assert_fixture(path: &str, fixture: &str, status: u16) {
    let s = common::start().await;
    let res = s
        .client
        .get(format!("{}{path}", s.base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), status, "{path}");
    assert_eq!(
        res.headers()["content-type"],
        "application/hal+json;charset=utf-8",
        "{path}"
    );
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(
        common::stable(body),
        common::stable(common::fixture(fixture, &s.base_url)),
        "{path}"
    );
}

#[tokio::test]
async fn pacticipants() {
    assert_fixture("/pacticipants", "GET_pacticipants.json", 200).await
}

#[tokio::test]
async fn environments() {
    assert_fixture("/environments", "GET_environments.json", 200).await
}

#[tokio::test]
async fn webhooks() {
    assert_fixture("/webhooks", "GET_webhooks.json", 200).await
}

#[tokio::test]
async fn integrations() {
    assert_fixture("/integrations", "GET_integrations.json", 200).await
}

#[tokio::test]
async fn latest_pacts() {
    assert_fixture("/pacts/latest", "GET_pacts_latest.json", 200).await
}

#[tokio::test]
async fn matrix_without_selectors() {
    assert_fixture("/matrix", "GET_matrix.json", 400).await
}

#[tokio::test]
async fn can_i_deploy_without_params() {
    assert_fixture("/can-i-deploy", "GET_can-i-deploy.json", 400).await
}
