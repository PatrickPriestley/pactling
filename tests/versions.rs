//! Versions, branches and tags: compared with the phase 2 fixtures captured from Ruby
//! broker 2.121.2 (tests/fixtures/ruby-broker/2.121.2/phase2/).
mod common;
use common::{fixture, stable};
use reqwest::StatusCode;
use serde_json::{Value, json};

const HAL: &str = "application/hal+json;charset=utf-8";
const JSON: &str = "application/json;charset=utf-8";

async fn put_json(s: &common::TestServer, path: &str, body: Value) -> reqwest::Response {
    s.client
        .put(format!("{}{path}", s.base_url))
        .header("content-type", "application/json")
        .header("accept", "application/hal+json")
        .json(&body)
        .send()
        .await
        .unwrap()
}

async fn put_empty(s: &common::TestServer, path: &str) -> reqwest::Response {
    s.client
        .put(format!("{}{path}", s.base_url))
        .header("content-type", "application/json")
        .send()
        .await
        .unwrap()
}

async fn get(s: &common::TestServer, path: &str) -> reqwest::Response {
    s.client
        .get(format!("{}{path}", s.base_url))
        .header("accept", "application/hal+json")
        .send()
        .await
        .unwrap()
}

async fn delete(s: &common::TestServer, path: &str) -> reqwest::Response {
    s.client
        .delete(format!("{}{path}", s.base_url))
        .send()
        .await
        .unwrap()
}

/// Publishes the fixture pact for Consumer/Provider at `version` through the legacy route.
async fn put_pact(s: &common::TestServer, version: &str) {
    let r = s
        .client
        .put(format!(
            "{}/pacts/provider/Provider/consumer/Consumer/version/{version}",
            s.base_url
        ))
        .header("content-type", "application/json")
        .body(include_str!("fixtures/pact/consumer-provider.json"))
        .send()
        .await
        .unwrap();
    assert!(r.status().is_success(), "{}", r.status());
}

fn content_type(r: &reqwest::Response) -> String {
    r.headers()
        .get("content-type")
        .map(|v| v.to_str().unwrap().to_owned())
        .unwrap_or_default()
}

fn phase2(name: &str, base: &str) -> Value {
    stable(fixture(&format!("phase2/{name}"), base))
}

#[tokio::test]
async fn put_version_creates_then_updates() {
    let s = common::start().await;
    let r = put_json(&s, "/pacticipants/Consumer/versions/8.0.0", json!({})).await;
    assert_eq!(r.status(), StatusCode::CREATED);
    assert_eq!(content_type(&r), HAL);
    let body: Value = r.json().await.unwrap();
    assert_eq!(
        stable(body),
        phase2("PUT_version_8.0.0_new.json", &s.base_url)
    );

    let r = put_json(
        &s,
        "/pacticipants/Consumer/versions/8.0.0",
        json!({"buildUrl": "https://ci/8"}),
    )
    .await;
    assert_eq!(r.status(), StatusCode::OK);
    let body: Value = r.json().await.unwrap();
    assert!(
        body.get("updatedAt").is_some(),
        "updated versions carry updatedAt"
    );
    assert_eq!(
        stable(body),
        phase2("PUT_version_8.0.0_existing.json", &s.base_url)
    );
}

#[tokio::test]
async fn put_version_creates_the_pacticipant_with_a_default_display_name() {
    let s = common::start().await;
    let r = put_json(&s, "/pacticipants/Brand_New/versions/1", json!({})).await;
    assert_eq!(r.status(), StatusCode::CREATED);
    let body: Value = r.json().await.unwrap();
    assert_eq!(
        stable(body),
        phase2("PUT_version_new_pacticipant.json", &s.base_url)
    );
    let p: Value = get(&s, "/pacticipants/Brand_New")
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(p["displayName"], "Brand New");
    assert_eq!(p["_embedded"]["latestVersion"]["number"], "1");
    assert_eq!(
        p["_embedded"]["latest-version"]["title"],
        "DEPRECATED - please use latestVersion"
    );
}

#[tokio::test]
async fn pacticipant_document_embeds_the_latest_version_and_detects_the_main_branch() {
    let s = common::start().await;
    put_empty(&s, "/pacticipants/Consumer/branches/main/versions/1.0.0").await;
    put_json(&s, "/pacticipants/Consumer/versions/10.0.0", json!({})).await;
    let r = get(&s, "/pacticipants/Consumer").await;
    let body: Value = r.json().await.unwrap();
    assert_eq!(
        stable(body),
        phase2("GET_pacticipants_Consumer.json", &s.base_url)
    );
}

#[tokio::test]
async fn main_branch_is_only_detected_from_develop_main_or_master_and_never_overwritten() {
    let s = common::start().await;
    put_empty(&s, "/pacticipants/A/branches/feature/versions/1").await;
    let a: Value = get(&s, "/pacticipants/A").await.json().await.unwrap();
    assert!(
        a.get("mainBranch").is_none(),
        "feature is not a main branch candidate"
    );
    put_empty(&s, "/pacticipants/A/branches/master/versions/2").await;
    let a: Value = get(&s, "/pacticipants/A").await.json().await.unwrap();
    assert_eq!(a["mainBranch"], "master");
    put_empty(&s, "/pacticipants/A/branches/main/versions/3").await;
    let a: Value = get(&s, "/pacticipants/A").await.json().await.unwrap();
    assert_eq!(a["mainBranch"], "master", "an existing main branch is kept");
}

#[tokio::test]
async fn unknown_version_tag_and_branch_are_the_plain_json_404() {
    let s = common::start().await;
    put_json(&s, "/pacticipants/Consumer/versions/1.0.0", json!({})).await;
    for path in [
        "/pacticipants/Consumer/versions/9.9.9",
        "/pacticipants/Consumer/versions/1.0.0/tags/nope",
        "/pacticipants/Consumer/versions/1.0.0/tags",
        "/pacticipants/Consumer/branches/nope",
        "/pacticipants/Consumer/branches/nope/latest-version",
        "/pacticipants/Consumer/branches/main/versions/9.9.9",
        "/pacticipants/Consumer/latest-version/nope",
    ] {
        let r = get(&s, path).await;
        assert_eq!(r.status(), StatusCode::NOT_FOUND, "{path}");
        assert_eq!(content_type(&r), JSON, "{path}");
        let body: Value = r.json().await.unwrap();
        assert_eq!(
            body,
            json!({"error": "The requested document was not found on this server."}),
            "{path}"
        );
    }
    let r = get(&s, "/pacticipants/Nope/latest-version").await;
    assert_eq!(r.status(), StatusCode::NOT_FOUND);
    // ...except the versions and branches lists of an unknown pacticipant, which name it.
    for path in ["/pacticipants/Nope/versions", "/pacticipants/Nope/branches"] {
        let r = get(&s, path).await;
        assert_eq!(r.status(), StatusCode::NOT_FOUND, "{path}");
        assert_eq!(content_type(&r), HAL, "{path}");
        assert_eq!(
            r.json::<Value>().await.unwrap(),
            json!({"error": "No pacticipant with name 'Nope' found"}),
            "{path}"
        );
    }
}

#[tokio::test]
async fn versions_list_is_newest_first_with_deprecated_links() {
    let s = common::start().await;
    put_json(&s, "/pacticipants/Consumer/versions/1.0.0", json!({})).await;
    put_json(&s, "/pacticipants/Consumer/versions/0.9.0", json!({})).await;
    let r = get(&s, "/pacticipants/Consumer/versions").await;
    assert_eq!(r.status(), StatusCode::OK);
    let body: Value = r.json().await.unwrap();
    let numbers: Vec<&str> = body["_embedded"]["versions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["number"].as_str().unwrap())
        .collect();
    assert_eq!(
        numbers,
        ["0.9.0", "1.0.0"],
        "creation order, newest first, not semver"
    );
    let first = &body["_embedded"]["versions"][0];
    assert!(
        first["_links"].get("pb:deployed-environments").is_none(),
        "list entries omit the environment links"
    );
    assert_eq!(first["_links"]["pb:pact-versions"], json!([]));
    let b = &s.base_url;
    let today = chrono::Utc::now().format("%d/%m/%Y").to_string();
    assert_eq!(
        body["_links"],
        json!({
            "self": { "href": format!("{b}/pacticipants/Consumer/versions"), "title": "All application versions of Consumer" },
            "pb:pacticipant": { "href": format!("{b}/pacticipants/Consumer"), "title": "Consumer" },
            "pb:versions": [
                { "href": format!("{b}/pacticipants/Consumer/versions/0.9.0"), "title": format!("Version 0.9.0 - {today}") },
                { "href": format!("{b}/pacticipants/Consumer/versions/1.0.0"), "title": format!("Version 1.0.0 - {today}") }
            ],
            "pacticipant": { "href": format!("{b}/pacticipants/Consumer"), "title": "Deprecated - please use pb:pacticipant" },
            "versions": [
                { "href": format!("{b}/pacticipants/Consumer/versions/0.9.0"), "title": "Deprecated - please use pb:versions" },
                { "href": format!("{b}/pacticipants/Consumer/versions/1.0.0"), "title": "Deprecated - please use pb:versions" }
            ]
        })
    );
}

#[tokio::test]
async fn provider_versions_list_is_empty_but_links_are_present() {
    let s = common::start().await;
    s.client
        .post(format!("{}/pacticipants", s.base_url))
        .json(&json!({"name": "Provider"}))
        .send()
        .await
        .unwrap();
    let r = get(&s, "/pacticipants/Provider/versions").await;
    assert_eq!(r.status(), StatusCode::OK);
    let body: Value = r.json().await.unwrap();
    assert_eq!(
        stable(body),
        phase2("GET_pacticipants_Provider_versions.json", &s.base_url)
    );
}

#[tokio::test]
async fn tag_put_creates_the_version_and_uses_the_first_tag_as_branch() {
    let s = common::start().await;
    let r = put_empty(&s, "/pacticipants/Consumer/versions/9.0.0/tags/t1").await;
    assert_eq!(r.status(), StatusCode::CREATED);
    assert_eq!(content_type(&r), HAL);
    assert_eq!(
        r.headers()["location"],
        format!(
            "{}/pacticipants/Consumer/versions/9.0.0/tags/t1",
            s.base_url
        )
        .as_str()
    );
    let body: Value = r.json().await.unwrap();
    assert_eq!(
        stable(body),
        phase2("PUT_tag_9.0.0_t1_new_version.json", &s.base_url)
    );

    let r = put_empty(&s, "/pacticipants/Consumer/versions/9.0.0/tags/t1").await;
    assert_eq!(r.status(), StatusCode::OK);
    assert!(r.headers().get("location").is_none());
    let body: Value = r.json().await.unwrap();
    assert_eq!(
        stable(body),
        phase2("PUT_tag_9.0.0_t1_same.json", &s.base_url)
    );

    put_empty(&s, "/pacticipants/Consumer/versions/9.0.0/tags/t2").await;
    let r = put_empty(&s, "/pacticipants/Consumer/branches/release/versions/9.0.0").await;
    assert_eq!(r.status(), StatusCode::CREATED);
    let body: Value = get(&s, "/pacticipants/Consumer/versions/9.0.0")
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(
        stable(body),
        phase2("GET_pacticipants_Consumer_versions_9.0.0.json", &s.base_url)
    );
}

#[tokio::test]
async fn tag_put_without_a_json_content_type_is_415() {
    let s = common::start().await;
    let r = s
        .client
        .put(format!(
            "{}/pacticipants/Consumer/versions/9.0.0/tags/t3",
            s.base_url
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
    assert_eq!(content_type(&r), HAL);
    assert_eq!(r.text().await.unwrap(), "");
    let r = get(&s, "/pacticipants/Consumer/versions/9.0.0").await;
    assert_eq!(r.status(), StatusCode::NOT_FOUND, "nothing was created");
}

#[tokio::test]
async fn tag_document_and_delete() {
    let s = common::start().await;
    put_pact(&s, "1.0.0").await;
    put_empty(&s, "/pacticipants/Consumer/branches/main/versions/1.0.0").await;
    put_empty(&s, "/pacticipants/Consumer/versions/1.0.0/tags/dev").await;
    // In the fixture a later version exists on main, so 1.0.0 is not the branch's latest.
    put_empty(&s, "/pacticipants/Consumer/branches/main/versions/2.0.0").await;
    let r = get(&s, "/pacticipants/Consumer/versions/1.0.0/tags/dev").await;
    assert_eq!(r.status(), StatusCode::OK);
    let body: Value = r.json().await.unwrap();
    assert_eq!(
        stable(body),
        phase2(
            "GET_pacticipants_Consumer_versions_1.0.0_tags_dev.json",
            &s.base_url
        )
    );
    let body: Value = get(&s, "/pacticipants/Consumer/versions/1.0.0")
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(
        stable(body),
        phase2("GET_pacticipants_Consumer_versions_1.0.0.json", &s.base_url)
    );

    assert_eq!(
        delete(&s, "/pacticipants/Consumer/versions/1.0.0/tags/dev")
            .await
            .status(),
        StatusCode::NO_CONTENT
    );
    let r = delete(&s, "/pacticipants/Consumer/versions/1.0.0/tags/dev").await;
    assert_eq!(r.status(), StatusCode::NOT_FOUND);
    assert_eq!(content_type(&r), JSON);
    let body: Value = get(&s, "/pacticipants/Consumer/versions/1.0.0")
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(body["_embedded"]["tags"], json!([]));
}

#[tokio::test]
async fn branch_version_put_creates_then_reports_existing() {
    let s = common::start().await;
    let r = put_empty(
        &s,
        "/pacticipants/Consumer/branches/release/versions/10.0.0",
    )
    .await;
    assert_eq!(r.status(), StatusCode::CREATED);
    assert_eq!(
        r.headers()["location"],
        format!(
            "{}/pacticipants/Consumer/branches/release/versions/10.0.0",
            s.base_url
        )
        .as_str()
    );
    let body: Value = r.json().await.unwrap();
    assert_eq!(
        stable(body),
        phase2("PUT_branch_version_release_10.0.0_new.json", &s.base_url)
    );
    let r = put_empty(
        &s,
        "/pacticipants/Consumer/branches/release/versions/10.0.0",
    )
    .await;
    assert_eq!(r.status(), StatusCode::OK);
    assert!(r.headers().get("location").is_none());
    let body: Value = r.json().await.unwrap();
    assert_eq!(
        stable(body),
        phase2("PUT_branch_version_release_10.0.0_same.json", &s.base_url)
    );

    let r = get(&s, "/pacticipants/Consumer/branches/release").await;
    assert_eq!(r.status(), StatusCode::OK);
    let body: Value = r.json().await.unwrap();
    assert_eq!(
        stable(body),
        phase2(
            "GET_pacticipants_Consumer_branches_release.json",
            &s.base_url
        )
    );
    let r = get(
        &s,
        "/pacticipants/Consumer/branches/release/versions/10.0.0",
    )
    .await;
    assert_eq!(r.status(), StatusCode::OK);
    let body: Value = r.json().await.unwrap();
    assert_eq!(body["_links"]["self"]["title"], "Branch version");
}

#[tokio::test]
async fn branches_list_is_paged_and_ordered_by_name() {
    let s = common::start().await;
    put_empty(&s, "/pacticipants/Consumer/branches/main/versions/1").await;
    put_empty(&s, "/pacticipants/Consumer/branches/feat/versions/2").await;
    let r = get(&s, "/pacticipants/Consumer/branches").await;
    assert_eq!(r.status(), StatusCode::OK);
    let body: Value = r.json().await.unwrap();
    assert_eq!(
        body["page"],
        json!({"number": 1, "size": 100, "totalElements": 2, "totalPages": 1})
    );
    let names: Vec<&str> = body["_embedded"]["branches"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["feat", "main"]);
    let b = &s.base_url;
    assert_eq!(
        body["_links"],
        json!({
            "self": { "title": "Consumer branches", "href": format!("{b}/pacticipants/Consumer/branches") },
            "pb:branches": [
                { "name": "feat", "href": format!("{b}/pacticipants/Consumer/branches/feat") },
                { "name": "main", "href": format!("{b}/pacticipants/Consumer/branches/main") }
            ]
        })
    );
    assert_eq!(
        body["_embedded"]["branches"][0]["_links"]["pb:latest-version"]["href"],
        format!("{b}/pacticipants/Consumer/branches/feat/latest-version")
    );
}

#[tokio::test]
async fn deleting_a_branch_keeps_its_versions() {
    let s = common::start().await;
    put_empty(
        &s,
        "/pacticipants/Consumer/branches/release/versions/10.0.0",
    )
    .await;
    assert_eq!(
        delete(
            &s,
            "/pacticipants/Consumer/branches/release/versions/10.0.0"
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    put_empty(
        &s,
        "/pacticipants/Consumer/branches/release/versions/10.0.0",
    )
    .await;
    assert_eq!(
        delete(&s, "/pacticipants/Consumer/branches/release")
            .await
            .status(),
        StatusCode::NO_CONTENT
    );
    let r = delete(&s, "/pacticipants/Consumer/branches/release").await;
    assert_eq!(r.status(), StatusCode::NOT_FOUND);
    assert_eq!(content_type(&r), JSON);
    let body: Value = get(&s, "/pacticipants/Consumer/versions/10.0.0")
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(
        stable(body),
        phase2(
            "AFTER_DELETES_GET_pacticipants_Consumer_versions_10.0.0.json",
            &s.base_url
        )
    );
}

#[tokio::test]
async fn deleting_a_version_removes_it() {
    let s = common::start().await;
    put_json(&s, "/pacticipants/Consumer/versions/9.0.0", json!({})).await;
    assert_eq!(
        delete(&s, "/pacticipants/Consumer/versions/9.0.0")
            .await
            .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        delete(&s, "/pacticipants/Consumer/versions/9.0.0")
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        get(&s, "/pacticipants/Consumer/versions/9.0.0")
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn latest_version_documents_echo_the_request_url_as_self() {
    let s = common::start().await;
    put_empty(&s, "/pacticipants/Consumer/versions/1.0.0/tags/dev").await;
    put_pact(&s, "2.0.1").await;
    put_empty(&s, "/pacticipants/Consumer/branches/main/versions/2.0.1").await;
    put_json(&s, "/pacticipants/Consumer/versions/3.0.0", json!({})).await;
    let b = &s.base_url;
    for (path, number) in [
        ("/pacticipants/Consumer/latest-version", "3.0.0"),
        ("/pacticipants/Consumer/latest-version/dev", "1.0.0"),
        (
            "/pacticipants/Consumer/branches/main/latest-version",
            "2.0.1",
        ),
    ] {
        let r = get(&s, path).await;
        assert_eq!(r.status(), StatusCode::OK, "{path}");
        let body: Value = r.json().await.unwrap();
        assert_eq!(body["number"], number, "{path}");
        assert_eq!(
            body["_links"]["self"]["href"],
            format!("{b}{path}"),
            "{path}"
        );
        assert_eq!(body["_links"]["self"]["name"], number);
        assert_eq!(
            body["_links"]["pb:tag"]["href"],
            format!("{b}/pacticipants/Consumer/versions/{number}/tags/{{tag}}")
        );
    }
    let body: Value = get(&s, "/pacticipants/Consumer/branches/main/latest-version")
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(
        stable(body),
        phase2(
            "GET_pacticipants_Consumer_branches_main_latest-version.json",
            &s.base_url
        )
    );
}

#[tokio::test]
async fn latest_version_of_a_pacticipant_without_versions_is_404() {
    let s = common::start().await;
    s.client
        .post(format!("{}/pacticipants", s.base_url))
        .json(&json!({"name": "Provider"}))
        .send()
        .await
        .unwrap();
    let r = get(&s, "/pacticipants/Provider/latest-version").await;
    assert_eq!(r.status(), StatusCode::NOT_FOUND);
    assert_eq!(content_type(&r), JSON);
}

#[tokio::test]
async fn options_advertise_the_ruby_brokers_methods() {
    let s = common::start().await;
    put_empty(&s, "/pacticipants/Consumer/branches/main/versions/1.0.0").await;
    put_empty(&s, "/pacticipants/Consumer/versions/1.0.0/tags/dev").await;
    for (path, allowed) in [
        (
            "/pacticipants/Consumer/versions/1.0.0",
            "GET, PUT, PATCH, DELETE, OPTIONS",
        ),
        (
            "/pacticipants/Consumer/versions/1.0.0/tags/dev",
            "GET, PUT, DELETE, OPTIONS",
        ),
        (
            "/pacticipants/Consumer/branches/main",
            "GET, DELETE, OPTIONS",
        ),
        (
            "/pacticipants/Consumer/branches/main/versions/1.0.0",
            "GET, PUT, DELETE, OPTIONS",
        ),
        ("/pacticipants/Consumer/versions", "GET, OPTIONS"),
        ("/pacticipants/Consumer/latest-version", "GET, OPTIONS"),
        ("/pacticipants/Consumer/branches", "GET, DELETE, OPTIONS"),
        (
            "/pacticipants/Consumer/branches/main/latest-version",
            "GET, OPTIONS",
        ),
    ] {
        let r = s
            .client
            .request(reqwest::Method::OPTIONS, format!("{}{path}", s.base_url))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::OK, "{path}");
        assert_eq!(
            r.headers()["access-control-allow-methods"],
            allowed,
            "{path}"
        );
        let r = s
            .client
            .request(reqwest::Method::HEAD, format!("{}{path}", s.base_url))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::METHOD_NOT_ALLOWED, "HEAD {path}");
        assert_eq!(r.headers()["allow"], allowed, "HEAD {path}");
    }
}
