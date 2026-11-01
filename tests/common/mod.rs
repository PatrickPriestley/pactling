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

/// One request a webhook receiver got.
#[derive(Clone, Debug)]
pub struct Received {
    pub method: String,
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

/// An in-process webhook receiver: records every request and answers with one status.
pub struct Receiver {
    pub base_url: String,
    pub received: std::sync::Arc<std::sync::Mutex<Vec<Received>>>,
}

impl Receiver {
    pub fn requests(&self) -> Vec<Received> {
        self.received.lock().unwrap().clone()
    }

    /// Waits up to `secs` seconds for `n` requests.
    pub async fn wait_for(&self, n: usize, secs: u64) -> Vec<Received> {
        for _ in 0..(secs * 20) {
            if self.received.lock().unwrap().len() >= n {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        self.requests()
    }
}

pub async fn receiver() -> Receiver {
    receiver_answering(200).await
}

pub async fn receiver_answering(status: u16) -> Receiver {
    use axum::{
        body::Bytes,
        extract::State,
        http::{HeaderMap, Method, StatusCode, Uri},
        routing::any,
    };
    let received = std::sync::Arc::new(std::sync::Mutex::new(Vec::<Received>::new()));
    let state = received.clone();
    let app = axum::Router::new()
        .fallback(any(
            move |State(st): State<std::sync::Arc<std::sync::Mutex<Vec<Received>>>>,
                  method: Method,
                  uri: Uri,
                  headers: HeaderMap,
                  body: Bytes| async move {
                let path = uri
                    .path_and_query()
                    .map(|p| p.as_str().to_owned())
                    .unwrap_or_default();
                st.lock().unwrap().push(Received {
                    method: method.to_string(),
                    path: path.clone(),
                    headers: headers
                        .iter()
                        .map(|(k, v)| (k.as_str().to_owned(), v.to_str().unwrap_or("").to_owned()))
                        .collect(),
                    body: String::from_utf8_lossy(&body).into_owned(),
                });
                let status = StatusCode::from_u16(status).unwrap();
                (
                    status,
                    [("content-type", "application/json")],
                    r#"{"received":true}"#,
                )
            },
        ))
        .with_state(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    Receiver {
        base_url: format!("http://{addr}"),
        received,
    }
}
