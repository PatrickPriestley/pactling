use pactling::db::Db;

#[tokio::test]
async fn migrations_create_pacticipants_table() {
    let db = Db::connect("sqlite::memory:").await.unwrap();
    db.migrate().await.unwrap();
    let names = db.list_pacticipant_names().await.unwrap();
    assert!(names.is_empty());
}

#[tokio::test]
async fn deleting_a_pacticipant_cascades_to_versions_branches_tags_and_pacts() {
    let db = Db::connect("sqlite::memory:").await.unwrap();
    db.migrate().await.unwrap();
    let pool = db.pool();
    sqlx::query("INSERT INTO pacticipants (id, name, created_at) VALUES (1, 'Consumer', 't'), (2, 'Provider', 't')")
        .execute(pool).await.unwrap();
    sqlx::query("INSERT INTO versions (id, pacticipant_id, number, created_at) VALUES (10, 1, '1.0.0', 't')")
        .execute(pool).await.unwrap();
    sqlx::query(
        "INSERT INTO branches (id, pacticipant_id, name, created_at) VALUES (20, 1, 'main', 't')",
    )
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO branch_versions (branch_id, version_id, created_at) VALUES (20, 10, 't')",
    )
    .execute(pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO tags (version_id, name, created_at) VALUES (10, 'dev', 't')")
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO pact_versions (id, consumer_id, provider_id, sha, content, created_at) VALUES (30, 1, 2, 'abc', '{}', 't')")
        .execute(pool).await.unwrap();
    sqlx::query("INSERT INTO pact_publications (consumer_version_id, provider_id, pact_version_id, created_at) VALUES (10, 2, 30, 't')")
        .execute(pool).await.unwrap();

    assert!(db.delete_pacticipant("Consumer").await.unwrap());

    for table in [
        "versions",
        "branches",
        "branch_versions",
        "tags",
        "pact_publications",
        "pact_versions",
    ] {
        let n = count(&db, table).await;
        assert_eq!(
            n, 0,
            "{table} should be empty after the consumer is deleted"
        );
    }
    assert_eq!(count(&db, "pacticipants").await, 1, "the provider stays");
}

#[tokio::test]
async fn a_pacticipant_cannot_have_two_versions_with_the_same_number() {
    let db = Db::connect("sqlite::memory:").await.unwrap();
    db.migrate().await.unwrap();
    let pool = db.pool();
    sqlx::query("INSERT INTO pacticipants (id, name, created_at) VALUES (1, 'Consumer', 't')")
        .execute(pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO versions (pacticipant_id, number, created_at) VALUES (1, '1.0.0', 't')",
    )
    .execute(pool)
    .await
    .unwrap();
    let dup = sqlx::query(
        "INSERT INTO versions (pacticipant_id, number, created_at) VALUES (1, '1.0.0', 't')",
    )
    .execute(pool)
    .await;
    assert!(dup.is_err(), "duplicate version number must be rejected");
}

async fn count(db: &Db, table: &str) -> i64 {
    // Table names cannot be bound, and sqlx 0.9 only takes static SQL, so match them.
    let sql = match table {
        "pacticipants" => "SELECT COUNT(*) FROM pacticipants",
        "versions" => "SELECT COUNT(*) FROM versions",
        "branches" => "SELECT COUNT(*) FROM branches",
        "branch_versions" => "SELECT COUNT(*) FROM branch_versions",
        "tags" => "SELECT COUNT(*) FROM tags",
        "pact_versions" => "SELECT COUNT(*) FROM pact_versions",
        "pact_publications" => "SELECT COUNT(*) FROM pact_publications",
        "verifications" => "SELECT COUNT(*) FROM verifications",
        other => panic!("unknown table {other}"),
    };
    sqlx::query_scalar(sql).fetch_one(db.pool()).await.unwrap()
}

#[tokio::test]
async fn verifications_cascade_from_pact_versions_and_provider_versions() {
    let db = Db::connect("sqlite::memory:").await.unwrap();
    db.migrate().await.unwrap();
    let pool = db.pool();
    sqlx::query(
        "INSERT INTO pacticipants (id, name, created_at) VALUES (1, 'C', 't'), (2, 'P', 't')",
    )
    .execute(pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO versions (id, pacticipant_id, number, created_at) VALUES (10, 1, '1', 't'), (20, 2, '1', 't')").execute(pool).await.unwrap();
    sqlx::query("INSERT INTO pact_versions (id, consumer_id, provider_id, sha, content, created_at) VALUES (30, 1, 2, 'abc', '{}', 't')").execute(pool).await.unwrap();
    sqlx::query("INSERT INTO verifications (number, pact_version_id, provider_version_id, consumer_version_id, success, created_at) VALUES (1, 30, 20, 10, 1, 't')").execute(pool).await.unwrap();
    assert_eq!(count(&db, "verifications").await, 1);
    db.delete_pacticipant("P").await.unwrap();
    assert_eq!(
        count(&db, "verifications").await,
        0,
        "deleting the provider removes its verifications"
    );
}
