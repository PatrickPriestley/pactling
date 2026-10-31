//! Pacticipant create, read, update, delete and list, matched against documents captured
//! from Ruby broker 2.121.2. Verified facts: the CLI creates with POST /pacticipants and
//! updates with PATCH; PATCH on a missing name creates (201); PUT on a missing name is 404;
//! POST on an existing name upserts and still answers 201; the POST Location header has
//! the Ruby broker's typo `/pacticpants/`; display names default to a title-cased
//! humanisation of the name; lists are ordered case-insensitively by name.
mod common;

use serde_json::{Value, json};

const JSON: &str = "application/json";

async fn post(s: &common::TestServer, body: Value) -> reqwest::Response {
    s.client
        .post(format!("{}/pacticipants", s.base_url))
        .header("content-type", JSON)
        .json(&body)
        .send()
        .await
        .unwrap()
}

async fn patch(s: &common::TestServer, name: &str, body: Value) -> reqwest::Response {
    s.client
        .patch(format!("{}/pacticipants/{name}", s.base_url))
        .header("content-type", "application/merge-patch+json")
        .json(&body)
        .send()
        .await
        .unwrap()
}

async fn get(s: &common::TestServer, path: &str) -> reqwest::Response {
    s.client
        .get(format!("{}{path}", s.base_url))
        .send()
        .await
        .unwrap()
}

fn public(v: &Value) -> Value {
    // Top-level data fields only, no links or embedded documents.
    Value::Object(
        v.as_object()
            .unwrap()
            .iter()
            .filter(|(k, _)| !k.starts_with('_'))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect(),
    )
}

#[tokio::test]
async fn post_creates_with_201_location_and_document() {
    let s = common::start().await;
    let res = post(
        &s,
        json!({"name":"Qux","displayName":"The Qux","repositoryUrl":"https://example.com/qux","mainBranch":"develop"}),
    )
    .await;
    assert_eq!(res.status(), 201);
    assert_eq!(
        res.headers()["content-type"],
        "application/hal+json;charset=utf-8"
    );
    // The Ruby broker's Location header really is spelled this way.
    assert_eq!(
        res.headers()["location"],
        format!("{}/pacticpants/Qux", s.base_url)
    );
    let body: Value = res.json().await.unwrap();
    assert_eq!(
        common::stable(body),
        common::stable(common::fixture("POST_pacticipants_Qux.json", &s.base_url))
    );
}

#[tokio::test]
async fn get_single_matches_fixture_including_deployed_environments_link() {
    let s = common::start().await;
    post(
        &s,
        json!({"name":"Qux","displayName":"The Qux","repositoryUrl":"https://example.com/qux","mainBranch":"develop"}),
    )
    .await;
    let res = get(&s, "/pacticipants/Qux").await;
    assert_eq!(res.status(), 200);
    let body: Value = res.json().await.unwrap();
    assert_eq!(
        common::stable(body),
        common::stable(common::fixture("GET_pacticipants_Qux.json", &s.base_url))
    );
}

#[tokio::test]
async fn post_existing_name_upserts_and_answers_201() {
    let s = common::start().await;
    post(
        &s,
        json!({"name":"Bar","mainBranch":"trunk","displayName":"The Bar"}),
    )
    .await;
    let res = post(
        &s,
        json!({"name":"Bar","repositoryUrl":"https://example.com/bar2"}),
    )
    .await;
    assert_eq!(res.status(), 201);
    let body: Value = res.json().await.unwrap();
    assert_eq!(
        body["mainBranch"], "trunk",
        "fields not in the second POST are kept"
    );
    assert_eq!(body["displayName"], "The Bar");
    assert_eq!(body["repositoryUrl"], "https://example.com/bar2");

    // Every column is independently kept when not provided.
    let res = post(&s, json!({"name":"Bar","mainBranch":"main"})).await;
    let body: Value = res.json().await.unwrap();
    assert_eq!(body["repositoryUrl"], "https://example.com/bar2");
    assert_eq!(body["displayName"], "The Bar");
    assert_eq!(body["mainBranch"], "main");
    let res = post(&s, json!({"name":"Bar","displayName":"Bar!"})).await;
    let body: Value = res.json().await.unwrap();
    assert_eq!(body["repositoryUrl"], "https://example.com/bar2");
    assert_eq!(body["mainBranch"], "main");
    assert_eq!(body["displayName"], "Bar!");
    assert!(
        body.get("updatedAt").is_some(),
        "an update carries updatedAt"
    );
}

#[tokio::test]
async fn post_validation_errors_match_ruby_broker() {
    let s = common::start().await;
    let res = post(&s, json!({"mainBranch":"main"})).await;
    assert_eq!(res.status(), 400);
    assert_eq!(
        res.headers()["content-type"],
        "application/hal+json;charset=utf-8"
    );
    let body: Value = res.json().await.unwrap();
    assert_eq!(body, json!({"errors":{"name":["is missing"]}}));

    let res = post(&s, json!({"name":""})).await;
    assert_eq!(res.status(), 400);
    let body: Value = res.json().await.unwrap();
    assert_eq!(body, json!({"errors":{"name":["must be filled"]}}));

    let res = s
        .client
        .post(format!("{}/pacticipants", s.base_url))
        .header("content-type", JSON)
        .body("{nope")
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 400);
    let body: Value = res.json().await.unwrap();
    let msg = body["error"].as_str().expect("error is a string");
    assert!(msg.starts_with("JSON::ParserError - "), "{msg}");
}

#[tokio::test]
async fn patch_creates_then_updates() {
    let s = common::start().await;
    let res = patch(&s, "Foo", json!({"displayName":"Foo Service"})).await;
    assert_eq!(res.status(), 201);
    let body: Value = res.json().await.unwrap();
    assert_eq!(
        common::stable(body),
        common::stable(common::fixture(
            "PATCH_pacticipants_Foo_created.json",
            &s.base_url
        ))
    );

    let res = patch(
        &s,
        "Foo",
        json!({"repositoryUrl":"https://example.com/foo","mainBranch":"main"}),
    )
    .await;
    assert_eq!(res.status(), 200);
    let body: Value = res.json().await.unwrap();
    assert!(body.get("updatedAt").is_some());
    assert_eq!(
        common::stable(body),
        common::stable(common::fixture(
            "PATCH_pacticipants_Foo_updated.json",
            &s.base_url
        ))
    );
}

#[tokio::test]
async fn patch_accepts_plain_json_content_type_and_ignores_unknown_fields() {
    let s = common::start().await;
    patch(&s, "Foo", json!({"displayName":"Foo Service"})).await;
    let res = s
        .client
        .patch(format!("{}/pacticipants/Foo", s.base_url))
        .header("content-type", JSON)
        .json(&json!({"bogus":1}))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    let body: Value = res.json().await.unwrap();
    assert_eq!(body["displayName"], "Foo Service");
    assert!(body.get("bogus").is_none());
}

#[tokio::test]
async fn put_updates_existing_but_is_404_for_missing() {
    let s = common::start().await;
    let res = s
        .client
        .put(format!("{}/pacticipants/Foo", s.base_url))
        .header("content-type", JSON)
        .json(&json!({"mainBranch":"main"}))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 404);
    let body: Value = res.json().await.unwrap();
    assert_eq!(
        body,
        json!({"error":"No pacticipant with name 'Foo' found"})
    );

    post(&s, json!({"name":"Foo"})).await;
    let res = s
        .client
        .put(format!("{}/pacticipants/Foo", s.base_url))
        .header("content-type", JSON)
        .json(&json!({"displayName":"Foo Service","repositoryUrl":"https://example.com/foo","mainBranch":"main"}))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    let body: Value = res.json().await.unwrap();
    assert_eq!(body["mainBranch"], "main");
}

#[tokio::test]
async fn delete_is_204_then_404() {
    let s = common::start().await;
    post(&s, json!({"name":"Foo"})).await;
    let res = s
        .client
        .delete(format!("{}/pacticipants/Foo", s.base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 204);
    assert_eq!(get(&s, "/pacticipants/Foo").await.status(), 404);
    let res = s
        .client
        .delete(format!("{}/pacticipants/Nope", s.base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 404);
    let body: Value = res.json().await.unwrap();
    assert_eq!(
        body,
        json!({"error":"No pacticipant with name 'Nope' found"})
    );
}

#[tokio::test]
async fn display_name_defaults_to_humanised_title_case() {
    let s = common::start().await;
    for (name, expected) in [
        ("my-service_v2", "My Service V2"),
        ("foo_bar-baz", "Foo Bar Baz"),
        ("ALLCAPS", "ALLCAPS"),
        ("camelCaseName", "Camel Case Name"),
        ("with.dots", "With Dots"),
        ("x", "X"),
        ("Example App", "Example App"),
    ] {
        let body: Value = post(&s, json!({"name": name})).await.json().await.unwrap();
        assert_eq!(body["displayName"], expected, "{name}");
    }
    let body: Value = post(&s, json!({"name":"Zed2","displayName":null}))
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(
        body["displayName"], "Zed2",
        "null displayName falls back to the default"
    );
    let body: Value = post(&s, json!({"name":"Zed","displayName":""}))
        .await
        .json()
        .await
        .unwrap();
    assert!(
        body.get("displayName").is_none(),
        "empty displayName is omitted"
    );
}

#[tokio::test]
async fn list_is_case_insensitively_sorted_and_matches_fixture() {
    let s = common::start().await;
    let fixture = common::fixture("GET_pacticipants_populated.json", &s.base_url);
    // Create exactly what the fixture holds, in reverse so ordering is the broker's doing.
    let entries = fixture["_embedded"]["pacticipants"].as_array().unwrap();
    for entry in entries.iter().rev() {
        let res = post(&s, public(entry)).await;
        assert_eq!(res.status(), 201);
    }
    let res = get(&s, "/pacticipants").await;
    assert_eq!(res.status(), 200);
    let body: Value = res.json().await.unwrap();
    let names: Vec<&str> = body["_embedded"]["pacticipants"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["Bar", "Baz", "Example App", "my-service_v2", "Qux"]);
    assert_eq!(common::stable(body), common::stable(fixture));
}

#[tokio::test]
async fn names_with_spaces_are_percent_encoded_in_links() {
    let s = common::start().await;
    post(&s, json!({"name":"Example App"})).await;
    let res = get(&s, "/pacticipants/Example%20App").await;
    assert_eq!(res.status(), 200);
    let body: Value = res.json().await.unwrap();
    assert_eq!(body["name"], "Example App");
    assert_eq!(
        body["_links"]["self"]["href"],
        format!("{}/pacticipants/Example%20App", s.base_url)
    );
    assert_eq!(
        common::stable(body),
        common::stable(common::fixture(
            "GET_pacticipants_Example_App.json",
            &s.base_url
        ))
    );
}

#[tokio::test]
async fn head_is_405_with_allow_and_options_is_200() {
    let s = common::start().await;
    post(&s, json!({"name":"Foo"})).await;
    let res = s
        .client
        .head(format!("{}/pacticipants/Foo", s.base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 405);
    assert_eq!(res.headers()["allow"], "GET, PUT, PATCH, DELETE, OPTIONS");
    let res = s
        .client
        .head(format!("{}/pacticipants", s.base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 405);
    let res = s
        .client
        .request(
            reqwest::Method::OPTIONS,
            format!("{}/pacticipants", s.base_url),
        )
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
}

#[tokio::test]
async fn patch_create_sets_a_correctly_spelled_location_and_updates_do_not() {
    let s = common::start().await;
    let res = patch(&s, "Foo", json!({"displayName":"Foo Service"})).await;
    assert_eq!(res.status(), 201);
    assert_eq!(
        res.headers()["location"],
        format!("{}/pacticipants/Foo", s.base_url)
    );
    let res = patch(&s, "Foo", json!({"mainBranch":"main"})).await;
    assert_eq!(res.status(), 200);
    assert!(res.headers().get("location").is_none());
    let res = s
        .client
        .put(format!("{}/pacticipants/Foo", s.base_url))
        .header("content-type", JSON)
        .json(&json!({"mainBranch":"main"}))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    assert!(res.headers().get("location").is_none());
}

#[tokio::test]
async fn concurrent_creates_of_one_name_all_succeed() {
    let s = common::start().await;
    let client = s.client.clone();
    let url = format!("{}/pacticipants", s.base_url);
    let mut tasks = Vec::new();
    for i in 0..32 {
        let client = client.clone();
        let url = url.clone();
        tasks.push(tokio::spawn(async move {
            client
                .post(&url)
                .header("content-type", JSON)
                .json(&json!({"name":"Race","mainBranch":format!("b{i}")}))
                .send()
                .await
                .unwrap()
                .status()
                .as_u16()
        }));
    }
    let mut statuses = Vec::new();
    for t in tasks {
        statuses.push(t.await.unwrap());
    }
    assert!(
        statuses.iter().all(|&st| st == 201),
        "every concurrent POST must be 201, got {statuses:?}"
    );
    let body: Value = get(&s, "/pacticipants/Race").await.json().await.unwrap();
    assert_eq!(body["name"], "Race");
}
