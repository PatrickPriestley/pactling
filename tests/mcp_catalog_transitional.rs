#![cfg(feature = "mcp")]
use serde_json::{Value, json};
use std::{
    process::Stdio,
    sync::{Arc, Mutex},
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, ChildStdout, Command},
    time::{Duration, timeout},
};

struct Session {
    child: Child,
    input: ChildStdin,
    output: tokio::io::Lines<BufReader<ChildStdout>>,
    seen: Arc<Mutex<Vec<String>>>,
    response: Arc<Mutex<(u16, String)>>,
    server: tokio::task::JoinHandle<()>,
}
impl Drop for Session {
    fn drop(&mut self) {
        self.server.abort();
        let _ = self.child.start_kill();
    }
}
impl Session {
    async fn new(status: u16, body: Value) -> Self {
        let seen = Arc::new(Mutex::new(vec![]));
        let response = Arc::new(Mutex::new((status, body.to_string())));
        let (capture, reply) = (seen.clone(), response.clone());
        let app = axum::Router::new().fallback(move |req: axum::extract::Request| {
            let (capture, reply) = (capture.clone(), reply.clone());
            async move {
                assert_eq!(req.method(), "GET");
                capture.lock().unwrap().push(req.uri().to_string());
                let (status, body) = reply.lock().unwrap().clone();
                axum::http::Response::builder()
                    .status(status)
                    .body(axum::body::Body::from(body))
                    .unwrap()
            }
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}/pr%C3%A9fix%20x", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let mut child = Command::new(env!("CARGO_BIN_EXE_pactling"))
            .args(["mcp", "--broker-base-url", &base])
            .env_remove("PACT_BROKER_USERNAME")
            .env_remove("PACT_BROKER_PASSWORD")
            .env_remove("PACT_BROKER_TOKEN")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let input = child.stdin.take().unwrap();
        let output = BufReader::new(child.stdout.take().unwrap()).lines();
        let mut s = Self {
            child,
            input,
            output,
            seen,
            response,
            server,
        };
        s.rpc("initialize",json!({"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"tool-test","version":"1"}})).await;
        s.input
            .write_all(b"{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n")
            .await
            .unwrap();
        s
    }
    async fn rpc(&mut self, method: &str, params: Value) -> Value {
        self.input
            .write_all(
                format!(
                    "{}\n",
                    json!({"jsonrpc":"2.0","id":2,"method":method,"params":params})
                )
                .as_bytes(),
            )
            .await
            .unwrap();
        self.input.flush().await.unwrap();
        let line = timeout(Duration::from_secs(8), self.output.next_line())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        serde_json::from_str(&line).unwrap()
    }
    async fn call(&mut self, name: &str, args: Value) -> Value {
        let r = self
            .rpc(
                "tools/call",
                json!({"name":format!("pactling_{name}"),"arguments":args}),
            )
            .await;
        assert!(
            r.get("error").is_none(),
            "known tool must return structured result: {r}"
        );
        let r = &r["result"];
        assert_eq!(r["content"].as_array().unwrap().len(), 1);
        assert_eq!(
            serde_json::from_str::<Value>(r["content"][0]["text"].as_str().unwrap()).unwrap(),
            r["structuredContent"]
        );
        assert_eq!(r["isError"], r["structuredContent"].get("error").is_some());
        r["structuredContent"].clone()
    }
    fn reply(&self, status: u16, body: Value) {
        *self.response.lock().unwrap() = (status, body.to_string());
    }
}
fn code(v: &Value, c: &str) {
    assert_eq!(v["error"]["code"], c, "{v}");
    assert_eq!(v["adapterSchemaVersion"], "1.0");
    assert_eq!(v.as_object().unwrap().len(), 2);
}

// Task 3 transitional contracts. Task 4 replaces this seam with frozen paging.
#[tokio::test]
async fn mcp_catalog_pages_freeze_one_response() {
    let mut s = Session::new(
        200,
        json!({"_embedded":{"pacticipants":[{"name":"A"},{"name":"B"}]}}),
    )
    .await;
    for size in [json!(1), json!(1.0)] {
        let r = s.call("list_pacticipants", json!({"pageSize":size})).await;
        assert!(r.get("data").is_some(), "expected successful page: {r}");
        assert_eq!(r["data"]["items"].as_array().unwrap().len(), 1);
        let cursor = r["data"]["nextCursor"].as_str().unwrap();
        let second = s.call("list_pacticipants", json!({"cursor":cursor})).await;
        assert_eq!(second["data"]["items"][0]["name"], "B");
        assert_eq!(second["data"]["nextCursor"], Value::Null);
        assert_eq!(r["data"]["retrievedAt"], second["data"]["retrievedAt"]);
        assert_eq!(
            s.call("list_pacticipants", json!({"cursor":cursor})).await,
            second
        );
    }
    s.reply(200, json!({"_embedded":{"pacticipants":[]}}));
    assert_eq!(
        s.call("list_pacticipants", json!({})).await["data"]["items"],
        json!([])
    );
}
#[tokio::test]
async fn mcp_cursor_with_page_size_is_invalid_input() {
    let mut s = Session::new(200, json!({"_embedded":{"integrations":[]}})).await;
    for args in [
        json!({"cursor":"valid","pageSize":2}),
        json!({"cursor":"valid","pageSize":"bad"}),
        json!({"cursor":false,"pageSize":null}),
    ] {
        let r = s.call("list_integrations", args).await;
        code(&r, "INVALID_INPUT");
        assert_eq!(
            r["error"]["details"],
            json!({"reasonCode":"SCHEMA_VALIDATION","field":"/cursor"})
        );
    }
    code(
        &s.call("list_integrations", json!({"cursor":"previous-process"}))
            .await,
        "CURSOR_INVALID",
    );
    assert!(s.seen.lock().unwrap().is_empty());
}
