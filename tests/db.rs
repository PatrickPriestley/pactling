use pactling::db::Db;

#[tokio::test]
async fn migrations_create_pacticipants_table() {
    let db = Db::connect("sqlite::memory:").await.unwrap();
    db.migrate().await.unwrap();
    let names = db.list_pacticipant_names().await.unwrap();
    assert!(names.is_empty());
}
