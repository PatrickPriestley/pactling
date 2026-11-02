use super::*;
use serde_json::json;
fn items(count: usize, bytes: usize) -> Vec<Value> {
    (0..count)
        .map(|n| json!({"name":format!("{n}{}","x".repeat(bytes))}))
        .collect()
}
async fn first(cache: &Pagination, count: usize, bytes: usize) -> Value {
    let result = cache
        .first(
            "pactling_list_pacticipants",
            items(count, bytes),
            1,
            &ReadRoute::Pacticipants,
            "https://broker/pacticipants",
        )
        .await;
    assert!(result.is_ok(), "expected cached response: {result:?}");
    result.unwrap()
}
#[tokio::test]
async fn monotonic_ttl_expires_without_refresh_on_replay_or_refetch() {
    let cache = Pagination::default();
    let page = first(&cache, 3, 1).await;
    let cursor = page["data"]["nextCursor"].as_str().unwrap();
    let created = Instant::now() - TTL + Duration::from_secs(2);
    cache.0.lock().unwrap()[0].created = created;
    assert!(cache.next("pactling_list_pacticipants", cursor).is_ok());
    assert_eq!(
        cache.0.lock().unwrap()[0].created,
        created,
        "replay must not refresh TTL"
    );
    cache.0.lock().unwrap()[0].created = Instant::now() - TTL;
    assert_eq!(
        cache
            .next("pactling_list_pacticipants", cursor)
            .unwrap_err()
            .0["error"]["code"],
        "CURSOR_EXPIRED"
    );
    assert!(
        cache.0.lock().unwrap().is_empty(),
        "expired entry must release its resources"
    );
    first(&cache, 3, 1).await;
    cache.0.lock().unwrap()[0].created = Instant::now() - TTL;
    first(&cache, 3, 1).await;
    assert_eq!(
        cache.0.lock().unwrap().len(),
        1,
        "first pages purge stale entries too"
    );
}
#[tokio::test]
async fn cache_budget_counts_serialized_metadata_and_evicts_oldest() {
    let cache = Pagination::default();
    let page = first(&cache, 3, 1).await;
    {
        let entries = cache.0.lock().unwrap();
        let serialized = serde_json::to_vec(&*entries).unwrap();
        assert_eq!(
            weight(&entries),
            serialized.len() + std::mem::size_of::<Instant>()
        );
        let fields: Value = serde_json::from_slice(&serialized).unwrap();
        for key in [
            "tool",
            "items",
            "page_size",
            "source",
            "retrieved_at",
            "pages",
        ] {
            assert!(fields[0].get(key).is_some(), "metadata absent: {key}");
        }
        assert!(fields[0]["pages"][1]["cursor"].is_string());
        assert!(fields[0]["pages"][1]["next"].is_string());
        assert_eq!(fields[0]["pages"][1]["start"], 1);
        assert_eq!(fields[0]["pages"][1]["end"], 2);
    }
    let cursor = page["data"]["nextCursor"].as_str().unwrap();
    // Each entry is about 4.5MiB. Four fit the count limit but exceed 16MiB.
    let mut large_cursors = vec![];
    for _ in 0..4 {
        let p = first(&cache, 20, 240_000).await;
        large_cursors.push(p["data"]["nextCursor"].as_str().unwrap().to_owned());
        assert!(weight(&cache.0.lock().unwrap()) <= CACHE_LIMIT);
    }
    assert_eq!(cache.0.lock().unwrap().len(), 3);
    assert!(cache.next("pactling_list_pacticipants", cursor).is_err());
    assert_eq!(
        cache
            .next("pactling_list_pacticipants", &large_cursors[0])
            .unwrap_err()
            .0["error"]["code"],
        "CURSOR_EXPIRED"
    );
    assert!(
        cache
            .next("pactling_list_pacticipants", &large_cursors[1])
            .is_ok()
    );
}
#[tokio::test]
async fn oversized_cache_entry_and_later_oversized_item_fail_atomically() {
    let cache = Pagination::default();
    let initial = first(&cache, 3, 1).await;
    let cursor = initial["data"]["nextCursor"].as_str().unwrap();
    let large = cache
        .first(
            "pactling_list_pacticipants",
            items(72, 240_000),
            1,
            &ReadRoute::Pacticipants,
            "https://broker/pacticipants",
        )
        .await;
    assert_eq!(large.unwrap_err().0["error"]["code"], "RESPONSE_TOO_LARGE");
    assert_eq!(cache.0.lock().unwrap().len(), 1);
    assert!(cache.next("pactling_list_pacticipants", cursor).is_ok());
    let later = cache
        .first(
            "pactling_list_pacticipants",
            vec![json!({"name":"small"}), json!({"name":"x".repeat(600_000)})],
            1,
            &ReadRoute::Pacticipants,
            "https://broker/pacticipants",
        )
        .await;
    assert_eq!(later.unwrap_err().0["error"]["code"], "RESPONSE_TOO_LARGE");
    assert_eq!(cache.0.lock().unwrap().len(), 1);
    assert!(cache.next("pactling_list_pacticipants", cursor).is_ok());
}
#[test]
fn result_budget_includes_escaped_text_and_errors_omit_only_oversized_optional_url() {
    let data = json!({"plugin":"\\\"".repeat(100_000)});
    let envelope = schema::success(data, "https://broker/pact");
    assert!(serde_json::to_vec(&envelope).unwrap().len() < RESULT_LIMIT);
    assert!(
        !fits(&envelope),
        "structured-only sizing misses escaped text"
    );
    let source = format!("https://broker/{}", "%E2%98%83".repeat(100_000));
    let result = error_result(ToolError::too_large(&ReadRoute::Pacticipants, &source));
    assert!(serde_json::to_vec(&result).unwrap().len() <= RESULT_LIMIT);
    let value = result.structured_content.unwrap();
    assert_eq!(value["error"]["code"], "RESPONSE_TOO_LARGE");
    assert!(
        value["error"].get("evidenceUrl").is_none(),
        "omit the whole optional URL, never truncate it"
    );
    let small = error_result(ToolError::too_large(
        &ReadRoute::Pacticipants,
        "https://broker/pacticipants",
    ));
    assert_eq!(
        small.structured_content.unwrap()["error"]["evidenceUrl"],
        "https://broker/pacticipants"
    );
}
