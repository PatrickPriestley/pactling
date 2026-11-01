#![allow(dead_code)]
use pactling::{app, config::Config};
use std::net::SocketAddr;

pub struct TestServer {
    pub base_url: String,
    pub client: reqwest::Client,
}

pub async fn start() -> TestServer {
    start_with(Config::in_memory()).await
}

pub async fn start_with(config: Config) -> TestServer {
    let router = app::build(config).await.expect("app builds");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr: SocketAddr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    TestServer {
        base_url: format!("http://{addr}"),
        client: reqwest::Client::new(),
    }
}

/// Load a Ruby-broker fixture and rewrite its hrefs to point at `base_url`.
pub fn fixture(name: &str, base_url: &str) -> serde_json::Value {
    let path = format!(
        "{}/tests/fixtures/ruby-broker/2.121.2/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    let raw = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    serde_json::from_str(&raw.replace("http://localhost:9292", base_url)).unwrap()
}

/// Remove volatile keys before comparing two documents.
pub fn stable(mut v: serde_json::Value) -> serde_json::Value {
    fn walk(v: &mut serde_json::Value) {
        match v {
            serde_json::Value::Object(m) => {
                m.shift_remove("createdAt");
                m.shift_remove("updatedAt");
                m.shift_remove("uuid");
                m.values_mut().for_each(walk);
            }
            serde_json::Value::Array(a) => a.iter_mut().for_each(walk),
            // "Version 1.0.0 - 31/10/2026": the link names carry the creation date.
            serde_json::Value::String(s) if is_dated_name(s) => {
                s.replace_range(s.len() - 10.., "DD/MM/YYYY");
            }
            _ => {}
        }
    }
    walk(&mut v);
    v
}

fn is_dated_name(s: &str) -> bool {
    let Some((head, date)) = s.rsplit_once(" - ") else {
        return false;
    };
    head.starts_with("Version ")
        && date.len() == 10
        && date.char_indices().all(|(i, c)| {
            if i == 2 || i == 5 {
                c == '/'
            } else {
                c.is_ascii_digit()
            }
        })
}
