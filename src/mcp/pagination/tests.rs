use super::*;
use serde_json::json;
#[tokio::test]
async fn concurrent_continuations_replay_identical_pages() {
    let cache = Pagination::default();
    let first = cache
        .first(
            "pactling_list_pacticipants",
            (0..7).map(|n| json!({"name":n})).collect(),
            2,
            &ReadRoute::Pacticipants,
            "https://broker/pacticipants",
        )
        .await;
    assert!(first.is_ok(), "missing frozen pages: {first:?}");
    let first = first.unwrap();
    let cursor = first["data"]["nextCursor"].as_str().unwrap().to_owned();
    let mut tasks = vec![];
    for _ in 0..20 {
        let (cache, cursor) = (cache.clone(), cursor.clone());
        tasks.push(tokio::spawn(async move {
            cache.next("pactling_list_pacticipants", &cursor).unwrap()
        }));
    }
    let expected = cache.next("pactling_list_pacticipants", &cursor).unwrap();
    for task in tasks {
        assert_eq!(task.await.unwrap(), expected);
    }
    assert_eq!(expected["data"]["items"], json!([{"name":2},{"name":3}]));
    assert!(expected["data"]["nextCursor"].is_string());
}
