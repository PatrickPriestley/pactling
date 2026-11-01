//! Latency budgets for the reads whose cost grew with version history (issue #6). The
//! dataset is seeded with SQL so the test measures the queries, not the HTTP layer.
use pactling::db::Db;
use std::time::{Duration, Instant};

const CONSUMERS: i64 = 20;
const PROVIDERS: i64 = 5;
const VERSIONS: i64 = 400;

/// 20 consumers on main, each with 400 versions publishing to 5 providers: 40,000 pact
/// publications sharing one pact content per integration.
async fn seeded() -> Db {
    let db = Db::connect("sqlite::memory:").await.unwrap();
    db.migrate().await.unwrap();
    let pool = db.pool();
    for sql in [
        format!("WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < {CONSUMERS})
                 INSERT INTO pacticipants (id, name, main_branch, created_at) SELECT i, printf('Consumer%03d', i), 'main', 't' FROM n"),
        format!("WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < {PROVIDERS})
                 INSERT INTO pacticipants (id, name, created_at) SELECT 1000 + i, printf('Provider%03d', i), 't' FROM n"),
        format!("WITH RECURSIVE c(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM c WHERE i < {CONSUMERS}),
                                v(j) AS (SELECT 1 UNION ALL SELECT j + 1 FROM v WHERE j < {VERSIONS})
                 INSERT INTO versions (id, pacticipant_id, number, created_at) SELECT (j - 1) * {CONSUMERS} + i, i, j, 't' FROM c, v"),
        "INSERT INTO branches (id, pacticipant_id, name, created_at) SELECT id, id, 'main', 't' FROM pacticipants WHERE id < 1000".to_owned(),
        "INSERT INTO branch_versions (branch_id, version_id, created_at) SELECT pacticipant_id, id, 't' FROM versions".to_owned(),
        "INSERT INTO pact_versions (consumer_id, provider_id, sha, content, created_at)
         SELECT c.id, p.id, printf('%040d', c.id * 10000 + p.id), '{\"interactions\":[]}', 't'
         FROM pacticipants c, pacticipants p WHERE c.id < 1000 AND p.id > 1000".to_owned(),
        "INSERT INTO pact_publications (consumer_version_id, provider_id, pact_version_id, created_at)
         SELECT v.id, pv.provider_id, pv.id, 't' FROM versions v JOIN pact_versions pv ON pv.consumer_id = v.pacticipant_id".to_owned(),
    ] {
        sqlx::query(sqlx::AssertSqlSafe(sql)).execute(pool).await.unwrap();
    }
    db
}

#[tokio::test]
async fn the_latest_pact_of_every_integration_is_found_without_rescanning_history() {
    let db = seeded().await;
    let t = Instant::now();
    let latest = db.latest_pacts().await.unwrap();
    let took = t.elapsed();
    assert_eq!(latest.len(), (CONSUMERS * PROVIDERS) as usize);
    assert!(
        latest
            .iter()
            .all(|p| p.consumer_version_number == VERSIONS.to_string())
    );
    assert!(
        took < Duration::from_millis(1000),
        "latest_pacts took {took:?}"
    );
}

#[tokio::test]
async fn the_latest_main_branch_pacts_of_a_provider_are_found_without_rescanning_history() {
    let db = seeded().await;
    let t = Instant::now();
    let latest = db.provider_pacts_latest_main_branch(1001).await.unwrap();
    let took = t.elapsed();
    assert_eq!(latest.len(), CONSUMERS as usize);
    assert!(
        latest
            .iter()
            .all(|p| p.consumer_version_number == VERSIONS.to_string())
    );
    assert!(
        took < Duration::from_millis(1000),
        "provider_pacts_latest_main_branch took {took:?}"
    );
}
