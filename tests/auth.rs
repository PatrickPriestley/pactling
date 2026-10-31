//! Basic auth behaves as the Ruby broker's does: 401 is plain text with an empty body and
//! `www-authenticate: Basic realm="Restricted area"`; the read-only user gets 401, not 403,
//! on writes. Captured in tests/fixtures/ruby-broker/2.121.2/auth/.
mod common;

use pactling::config::Config;

fn secured() -> Config {
    Config {
        basic_auth_username: Some("rw".into()),
        basic_auth_password: Some("rwpass".into()),
        basic_auth_read_only_username: Some("ro".into()),
        basic_auth_read_only_password: Some("ropass".into()),
        ..Config::in_memory()
    }
}

async fn assert_401(res: reqwest::Response) {
    assert_eq!(res.status(), 401);
    assert_eq!(
        res.headers()["www-authenticate"],
        "Basic realm=\"Restricted area\""
    );
    assert_eq!(res.headers()["content-type"], "text/plain");
    assert_eq!(res.text().await.unwrap(), "");
}

#[tokio::test]
async fn no_credentials_is_401() {
    let s = common::start_with(secured()).await;
    assert_401(s.client.get(&s.base_url).send().await.unwrap()).await;
}

#[tokio::test]
async fn wrong_password_is_401() {
    let s = common::start_with(secured()).await;
    let res = s
        .client
        .get(&s.base_url)
        .basic_auth("rw", Some("wrong"))
        .send()
        .await
        .unwrap();
    assert_401(res).await;
}

#[tokio::test]
async fn options_without_credentials_is_401() {
    let s = common::start_with(secured()).await;
    let res = s
        .client
        .request(reqwest::Method::OPTIONS, &s.base_url)
        .send()
        .await
        .unwrap();
    assert_401(res).await;
}

#[tokio::test]
async fn read_only_user_can_read() {
    let s = common::start_with(secured()).await;
    let res = s
        .client
        .get(&s.base_url)
        .basic_auth("ro", Some("ropass"))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
}

#[tokio::test]
async fn read_only_user_cannot_write() {
    let s = common::start_with(secured()).await;
    for method in [
        reqwest::Method::PUT,
        reqwest::Method::POST,
        reqwest::Method::PATCH,
        reqwest::Method::DELETE,
    ] {
        let res = s
            .client
            .request(method.clone(), format!("{}/pacticipants/X", s.base_url))
            .basic_auth("ro", Some("ropass"))
            .header("content-type", "application/json")
            .body("{}")
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), 401, "{method}");
    }
}

#[tokio::test]
async fn read_write_user_passes_auth_on_writes() {
    let s = common::start_with(secured()).await;
    let res = s
        .client
        .post(format!("{}/no-such-route", s.base_url))
        .basic_auth("rw", Some("rwpass"))
        .send()
        .await
        .unwrap();
    // Past auth: the request reaches routing and gets the broker's own 404.
    assert_eq!(res.status(), 404);
}

#[tokio::test]
async fn public_read_allows_anonymous_get_but_not_write() {
    let s = common::start_with(Config {
        allow_public_read: true,
        ..secured()
    })
    .await;
    assert_eq!(
        s.client.get(&s.base_url).send().await.unwrap().status(),
        200
    );
    let opt = s
        .client
        .request(reqwest::Method::OPTIONS, &s.base_url)
        .send()
        .await
        .unwrap();
    assert_eq!(opt.status(), 200);
    let put = s
        .client
        .put(format!("{}/pacticipants/Bar", s.base_url))
        .header("content-type", "application/json")
        .body("{}")
        .send()
        .await
        .unwrap();
    assert_401(put).await;
}

#[tokio::test]
async fn empty_credentials_mean_no_auth() {
    // Compose files pass unset variables through as empty strings.
    let s = common::start_with(Config {
        basic_auth_username: Some(String::new()),
        basic_auth_password: Some(String::new()),
        ..Config::in_memory()
    })
    .await;
    assert_eq!(
        s.client.get(&s.base_url).send().await.unwrap().status(),
        200
    );
}

#[tokio::test]
async fn bearer_token_grants_read_and_write() {
    let s = common::start_with(Config {
        bearer_token: Some("tok".into()),
        ..secured()
    })
    .await;
    let get = s
        .client
        .get(&s.base_url)
        .bearer_auth("tok")
        .send()
        .await
        .unwrap();
    assert_eq!(get.status(), 200);
    let post = s
        .client
        .post(format!("{}/no-such-route", s.base_url))
        .bearer_auth("tok")
        .send()
        .await
        .unwrap();
    assert_eq!(post.status(), 404);
    let bad = s
        .client
        .get(&s.base_url)
        .bearer_auth("nope")
        .send()
        .await
        .unwrap();
    assert_401(bad).await;
}

#[tokio::test]
async fn right_password_with_wrong_username_is_401() {
    let s = common::start_with(secured()).await;
    for (user, pass) in [("someone", "rwpass"), ("rw", "ropass"), ("ro", "rwpass")] {
        let res = s
            .client
            .get(&s.base_url)
            .basic_auth(user, Some(pass))
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), 401, "{user}:{pass}");
    }
}
