//! Business operations are atomic (issue #4): a refused publish leaves no metadata behind,
//! concurrent deployments cannot both be current, and a dropped transaction writes nothing.
mod common;
use base64::Engine;
use reqwest::StatusCode;
use serde_json::{Value, json};

fn pact(status: u16) -> Value {
    let mut p: Value =
        serde_json::from_str(include_str!("fixtures/pact/consumer-provider.json")).unwrap();
    p["interactions"][0]["response"]["status"] = json!(status);
    p
}

fn publish_body(version: &str, tag: &str, content: &Value) -> Value {
    json!({
        "pacticipantName": "Consumer", "pacticipantVersionNumber": version, "branch": "main", "tags": [tag],
        "contracts": [{
            "consumerName": "Consumer", "providerName": "Provider", "specification": "pact", "contentType": "application/json",
            "content": base64::engine::general_purpose::STANDARD.encode(serde_json::to_vec(content).unwrap())
        }]
    })
}

#[tokio::test]
async fn a_refused_concurrent_publish_leaves_none_of_its_metadata() {
    let s = common::start().await;
    concurrent_publishes_leave_one_winner(&s).await;
}

/// The same race on a file database with several pooled connections, where a deferred
/// transaction would fail with "database is locked" on lock upgrade instead of waiting.
#[tokio::test]
async fn concurrent_publishes_on_a_file_database_wait_for_each_other() {
    let dir = std::env::temp_dir().join(format!("pactling-tx-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut config = pactling::config::Config::in_memory();
    config.database_url = format!("sqlite://{}/broker.db?mode=rwc", dir.display());
    let s = common::start_with(config).await;
    concurrent_publishes_leave_one_winner(&s).await;
    std::fs::remove_dir_all(&dir).ok();
}

async fn concurrent_publishes_leave_one_winner(s: &common::TestServer) {
    for i in 0..10 {
        let version = format!("r{i}");
        let publish = |tag: &'static str, status: u16| {
            let client = s.client.clone();
            let url = format!("{}/contracts/publish", s.base_url);
            let body = publish_body(&version, tag, &pact(status));
            async move { client.post(url).json(&body).send().await.unwrap().status() }
        };
        let (a, b) = tokio::join!(publish("alpha", 200), publish("beta", 201));
        let statuses = [a, b];
        assert!(
            statuses.contains(&StatusCode::OK) && statuses.contains(&StatusCode::CONFLICT),
            "round {i}: one wins and one is refused, got {statuses:?}"
        );
        let winner = if a == StatusCode::OK { "alpha" } else { "beta" };
        let doc: Value = s
            .client
            .get(format!(
                "{}/pacticipants/Consumer/versions/{version}",
                s.base_url
            ))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let tags: Vec<&str> = doc["_embedded"]["tags"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert_eq!(tags, [winner], "round {i}: only the winner's tag may exist");
    }
}

#[tokio::test]
async fn concurrent_deployments_leave_one_current_record() {
    let s = common::start().await;
    let env: Value = s
        .client
        .post(format!("{}/environments", s.base_url))
        .json(&json!({"name": "test", "production": false}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let env_uuid = env["uuid"].as_str().unwrap().to_owned();
    for i in 0..10 {
        for v in ["1", "2"] {
            let r = s
                .client
                .put(format!("{}/pacticipants/App{i}/versions/{v}", s.base_url))
                .header("content-type", "application/json")
                .body("{}")
                .send()
                .await
                .unwrap();
            assert!(r.status().is_success());
        }
        let deploy = |v: &'static str| {
            let client = s.client.clone();
            let url = format!(
                "{}/pacticipants/App{i}/versions/{v}/deployed-versions/environment/{env_uuid}",
                s.base_url
            );
            async move {
                client
                    .post(url)
                    .json(&json!({}))
                    .send()
                    .await
                    .unwrap()
                    .status()
            }
        };
        let (a, b) = tokio::join!(deploy("1"), deploy("2"));
        assert!(a.is_success() && b.is_success(), "round {i}: {a} {b}");
        let current: Value = s.client.get(format!("{}/environments/{env_uuid}/deployed-versions/currently-deployed?pacticipant=App{i}", s.base_url)).send().await.unwrap().json().await.unwrap();
        let n = current["_embedded"]["deployedVersions"]
            .as_array()
            .unwrap()
            .len();
        assert_eq!(
            n, 1,
            "round {i}: exactly one version of App{i} may be current, found {n}"
        );
    }
}

#[tokio::test]
async fn a_dropped_transaction_writes_nothing() {
    let db = pactling::db::Db::connect("sqlite::memory:").await.unwrap();
    db.migrate().await.unwrap();
    {
        let tx = db.begin().await.unwrap();
        pactling::service::versions::put(&tx, "Ghost", "1", None)
            .await
            .unwrap();
        assert!(
            tx.find_pacticipant("Ghost").await.unwrap().is_some(),
            "visible inside the transaction"
        );
        // dropped without commit
    }
    assert!(
        db.find_pacticipant("Ghost").await.unwrap().is_none(),
        "rolled back"
    );
    let tx = db.begin().await.unwrap();
    pactling::service::versions::put(&tx, "Real", "1", None)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert!(
        db.find_pacticipant("Real").await.unwrap().is_some(),
        "committed"
    );
}

#[tokio::test]
async fn file_databases_run_in_wal_mode() {
    // Review finding: assert the journal mode rather than trusting the connect options.
    let dir = std::env::temp_dir().join(format!("pactling-tx-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let db = pactling::db::Db::connect(&format!("sqlite://{}/broker.db?mode=rwc", dir.display()))
        .await
        .unwrap();
    let mode: String = sqlx::query_scalar("PRAGMA journal_mode")
        .fetch_one(db.pool())
        .await
        .unwrap();
    assert_eq!(mode, "wal");
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn concurrent_upserts_of_a_new_webhook_both_succeed() {
    // Review finding: the lookup deciding between create and update must be inside the
    // transaction, or two PUTs for a missing uuid both try to create.
    let mut config = pactling::config::Config::in_memory();
    config.webhook_scheme_whitelist = vec!["http".into()];
    let s = common::start_with(config).await;
    for i in 0..10 {
        let put = |description: &'static str| {
            let client = s.client.clone();
            let url = format!("{}/webhooks/pactling-upsert-race-{i:02}", s.base_url);
            async move {
                client.put(url).header("content-type", "application/json").json(&json!({"description": description, "request": {"method": "POST", "url": "http://example.com/hook"}})).send().await.unwrap().status()
            }
        };
        let (a, b) = tokio::join!(put("first"), put("second"));
        let statuses = [a, b];
        assert!(
            statuses.contains(&StatusCode::CREATED) && statuses.contains(&StatusCode::OK),
            "round {i}: one creates and the other updates, got {statuses:?}"
        );
    }
}

#[tokio::test]
async fn committing_while_a_shared_handle_is_alive_is_an_error() {
    // Review finding: the owner must not report success while an inner handle could still
    // roll the transaction back.
    let db = pactling::db::Db::connect("sqlite::memory:").await.unwrap();
    db.migrate().await.unwrap();
    let tx = db.begin().await.unwrap();
    let inner = tx.begin().await.unwrap();
    pactling::service::versions::put(&inner, "Shared", "1", None)
        .await
        .unwrap();
    assert!(
        inner.commit().await.is_ok(),
        "an inner handle's commit is a no-op"
    );
    let inner2 = tx.begin().await.unwrap();
    let err = tx.commit().await;
    assert!(
        err.is_err(),
        "the owner cannot commit while a handle is alive"
    );
    drop(inner2);
    // The transaction was consumed by the failed commit attempt only if it succeeded; here
    // it rolled back, so nothing was written.
    assert!(db.find_pacticipant("Shared").await.unwrap().is_none());
}
