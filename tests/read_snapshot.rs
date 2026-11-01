//! Real SQLx/SQLite probes and read-snapshot regressions.
mod common;
use pactling::{config::Config, db::Db, service};
use serde_json::json;
use sqlx::sqlite::SqlitePoolOptions;
use std::time::Duration;

#[tokio::test]
async fn sqlite_read_snapshot_protocol_probe() {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    let mut conn = pool.acquire().await.unwrap();
    conn.close_on_drop();
    sqlx::query("CREATE TABLE probe (id INTEGER)")
        .execute(&mut *conn)
        .await
        .unwrap();
    sqlx::query("INSERT INTO probe VALUES (1)")
        .execute(&mut *conn)
        .await
        .unwrap();
    let version: String = sqlx::query_scalar("SELECT sqlite_version()")
        .fetch_one(&mut *conn)
        .await
        .unwrap();
    let no_tx = sqlx::query("ROLLBACK")
        .execute(&mut *conn)
        .await
        .unwrap_err();
    let error = no_tx.as_database_error().unwrap();
    assert_eq!(error.code().as_deref(), Some("1"));
    assert_eq!(
        error.message(),
        "cannot rollback - no transaction is active"
    );
    sqlx::query("PRAGMA query_only = ON")
        .execute(&mut *conn)
        .await
        .unwrap();
    sqlx::query("BEGIN").execute(&mut *conn).await.unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM probe")
            .fetch_one(&mut *conn)
            .await
            .unwrap(),
        1
    );
    let denied = sqlx::query("INSERT INTO probe VALUES (2)")
        .execute(&mut *conn)
        .await
        .unwrap_err();
    assert_eq!(
        denied.as_database_error().unwrap().code().as_deref(),
        Some("8")
    );
    sqlx::query("ROLLBACK").execute(&mut *conn).await.unwrap();
    sqlx::query("PRAGMA query_only = OFF")
        .execute(&mut *conn)
        .await
        .unwrap();
    conn.return_to_pool().await;
    drop(conn);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("PRAGMA query_only")
            .fetch_one(&pool)
            .await
            .unwrap(),
        0
    );
    sqlx::query("INSERT INTO probe VALUES (2)")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM probe")
            .fetch_one(&pool)
            .await
            .unwrap(),
        2
    );
    println!(
        "SQLx 0.9.0 / SQLite {version}: deferred BEGIN, query_only denial/reset, no-transaction rollback and explicit pool return verified"
    );
}

async fn memory() -> Db {
    let db = Db::connect("sqlite::memory:").await.unwrap();
    db.migrate().await.unwrap();
    service::versions::put(&db, "Sentinel", "1", None)
        .await
        .unwrap();
    db
}

async fn assert_writable(db: &Db, version: &str) {
    tokio::time::timeout(Duration::from_secs(2), async {
        assert!(
            db.find_version("Sentinel", "1").await.unwrap().is_some(),
            "snapshot cleanup must preserve the in-memory connection"
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("PRAGMA query_only")
                .fetch_one(db.pool())
                .await
                .unwrap(),
            0
        );
        service::versions::put(db, "Sentinel", version, None)
            .await
            .unwrap();
    })
    .await
    .expect("snapshot cleanup released the pooled connection");
}

#[tokio::test]
async fn snapshot_works_with_one_connection_and_rejects_writes() {
    let db = memory().await;
    let snapshot = db.begin_read().await.unwrap();
    assert!(
        snapshot
            .find_version("Sentinel", "1")
            .await
            .unwrap()
            .is_some()
    );
    // This mutator executes SQL directly, without calling Db::begin first.
    let denied = snapshot
        .upsert_pacticipant("Forbidden", None, "Forbidden", None, None)
        .await
        .unwrap_err();
    assert_eq!(
        denied.as_database_error().unwrap().code().as_deref(),
        Some("8")
    );
    assert!(
        snapshot
            .find_pacticipant("Forbidden")
            .await
            .unwrap()
            .is_none()
    );
    snapshot.commit().await.unwrap();
    assert_writable(&db, "2").await;
}

#[tokio::test]
async fn begin_read_rejects_existing_write_transaction_and_read_upgrade() {
    let db = memory().await;
    let tx = db.begin().await.unwrap();
    assert!(
        tx.begin_read().await.is_err(),
        "read snapshot cannot share a write transaction"
    );
    tx.commit().await.unwrap();
    let read = db.begin_read().await.unwrap();
    assert!(
        read.begin().await.is_err(),
        "read snapshot cannot upgrade to a write transaction"
    );
    assert!(
        read.begin_read().await.is_err(),
        "nested read snapshots must not acquire from the one-connection pool"
    );
    read.commit().await.unwrap();
    assert_writable(&db, "2").await;
}

#[tokio::test]
async fn snapshot_cleanup_on_drop_error_and_cancellation() {
    let db = memory().await;
    let read = db.begin_read().await.unwrap();
    drop(read);
    assert_writable(&db, "2").await;
    let read = db.begin_read().await.unwrap();
    assert!(read.delete_pacticipant("Sentinel").await.is_err());
    drop(read);
    assert_writable(&db, "3").await;

    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
    let task_db = db.clone();
    let task = tokio::spawn(async move {
        let read = task_db.begin_read().await.unwrap();
        ready_tx.send(()).unwrap();
        std::future::pending::<()>().await;
        drop(read);
    });
    ready_rx.await.unwrap();
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    assert_writable(&db, "4").await;

    // Exercise cancellation while acquiring, enabling query_only, opening BEGIN,
    // and pinning the initial data read, rather than only after begin_read returns.
    for yields in 0..12 {
        let task_db = db.clone();
        let task = tokio::spawn(async move { task_db.begin_read().await });
        for _ in 0..yields {
            tokio::task::yield_now().await;
        }
        task.abort();
        if let Ok(Ok(read)) = task.await {
            drop(read);
        }
        assert_writable(&db, &format!("cancel-{yields}")).await;
    }
}

#[tokio::test]
async fn read_snapshot_owner_cannot_finish_while_borrowed_clone_lives() {
    let db = memory().await;
    let read = db.begin_read().await.unwrap();
    let copy = read.clone();
    copy.commit().await.unwrap();
    assert!(read.find_version("Sentinel", "1").await.unwrap().is_some());
    let copy = read.clone();
    assert!(
        read.commit().await.is_err(),
        "owner must not finish a live shared read handle"
    );
    assert!(copy.find_version("Sentinel", "1").await.unwrap().is_some());
    assert!(copy.delete_pacticipant("Sentinel").await.is_err());
    drop(copy);
    assert_writable(&db, "2").await;
}

async fn file_database() -> (Db, common::TestServer, std::path::PathBuf) {
    let path = std::env::temp_dir().join(format!("pactling-snapshot-{}.db", uuid::Uuid::new_v4()));
    let mut config = Config::in_memory();
    config.database_url = format!("sqlite://{}?mode=rwc", path.display());
    let db = Db::connect(&config.database_url).await.unwrap();
    let server = common::start_with(config).await;
    let journal: String = sqlx::query_scalar("PRAGMA journal_mode")
        .fetch_one(db.pool())
        .await
        .unwrap();
    assert_eq!(journal, "wal");
    (db, server, path)
}

async fn publish(s: &common::TestServer, version: &str) {
    let response = s
        .client
        .put(format!(
            "{}/pacts/provider/P/consumer/C/version/{version}",
            s.base_url
        ))
        .json(&json!({"consumer":{"name":"C"},"provider":{"name":"P"},"interactions":[]}))
        .send()
        .await
        .unwrap();
    assert!(
        response.status().is_success(),
        "publish: {}",
        response.status()
    );
}

#[tokio::test]
async fn read_snapshot_does_not_block_publish_and_sees_all_before_or_after() {
    let (db, server, path) = file_database().await;
    publish(&server, "1").await;
    let consumer = db.find_pacticipant("C").await.unwrap().unwrap();
    let read = db.begin_read().await.unwrap();
    // Opening and first data read form the barrier. The writer must finish while
    // this owner is still held, not after a reader task quietly releases it.
    tokio::time::timeout(Duration::from_secs(2), publish(&server, "2"))
        .await
        .expect("publish completed before the read snapshot was released");
    assert!(read.find_version("C", "2").await.unwrap().is_none());
    assert_eq!(
        read.publications_for_consumer(consumer.id)
            .await
            .unwrap()
            .len(),
        1
    );
    assert_eq!(read.versions_of(consumer.id).await.unwrap().len(), 1);
    read.commit().await.unwrap();
    // Inverse schedule: writer committed before this snapshot's first read.
    let read = db.begin_read().await.unwrap();
    assert!(read.find_version("C", "2").await.unwrap().is_some());
    assert_eq!(
        read.publications_for_consumer(consumer.id)
            .await
            .unwrap()
            .len(),
        2
    );
    assert_eq!(read.versions_of(consumer.id).await.unwrap().len(), 2);
    read.commit().await.unwrap();
    publish(&server, "3").await;
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn read_snapshot_does_not_block_deployment_and_sees_all_before_or_after() {
    let (db, _server, path) = file_database().await;
    let (_, _) = service::versions::put(&db, "P", "1", None).await.unwrap();
    service::versions::put(&db, "P", "2", None).await.unwrap();
    let provider = db.find_pacticipant("P").await.unwrap().unwrap();
    let first = db.find_version("P", "1").await.unwrap().unwrap();
    let second = db.find_version("P", "2").await.unwrap().unwrap();
    let env = db
        .create_environment("test", None, false, None)
        .await
        .unwrap();
    service::environments::record_deployment(&db, "P", "1", &env.uuid, Some("a"))
        .await
        .unwrap();
    let read = db.begin_read().await.unwrap();
    tokio::time::timeout(
        Duration::from_secs(2),
        service::environments::record_deployment(&db, "P", "2", &env.uuid, Some("a")),
    )
    .await
    .expect("deployment committed before releasing the read snapshot")
    .unwrap();
    assert_eq!(
        read.version_ids_in_environment(provider.id, env.id)
            .await
            .unwrap(),
        [first.id]
    );
    let rows = read.currently_deployed(env.id, Some("P")).await.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].version_number, "1");
    assert!(
        read.deployments_of_version(second.id)
            .await
            .unwrap()
            .is_empty()
    );
    read.commit().await.unwrap();
    let read = db.begin_read().await.unwrap();
    assert_eq!(
        read.version_ids_in_environment(provider.id, env.id)
            .await
            .unwrap(),
        [second.id]
    );
    let rows = read.currently_deployed(env.id, Some("P")).await.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].version_number, "2");
    assert!(!read.deployments_of_version(first.id).await.unwrap()[0].currently_deployed);
    read.commit().await.unwrap();
    service::environments::record_deployment(&db, "P", "1", &env.uuid, Some("a"))
        .await
        .unwrap();
    std::fs::remove_file(path).unwrap();
}
