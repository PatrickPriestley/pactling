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

fn participants(count: usize) -> Value {
    json!({"_embedded":{"pacticipants":(0..count).map(|n|json!({"name":format!("P{n}")})).collect::<Vec<_>>()}})
}
#[tokio::test]
async fn mcp_nondefault_page_size_persists_across_three_pages() {
    let mut s = Session::new(200, participants(8)).await;
    let first = s.call("list_pacticipants", json!({"pageSize":3})).await;
    s.reply(200, participants(1));
    assert!(
        first.get("data").is_some(),
        "expected successful page: {first}"
    );
    let mut page = first.clone();
    let mut names = vec![];
    for count in [3, 3, 2] {
        assert_eq!(page["data"]["items"].as_array().unwrap().len(), count);
        names.extend(
            page["data"]["items"]
                .as_array()
                .unwrap()
                .iter()
                .map(|x| x["name"].clone()),
        );
        if let Some(cursor) = page["data"]["nextCursor"].as_str() {
            let next = s.call("list_pacticipants", json!({"cursor":cursor})).await;
            assert_eq!(
                next,
                s.call("list_pacticipants", json!({"cursor":cursor})).await
            );
            page = next;
        }
    }
    assert_eq!(page["data"]["nextCursor"], Value::Null);
    assert_eq!(
        names,
        (0..8).map(|n| json!(format!("P{n}"))).collect::<Vec<_>>()
    );
    assert_eq!(s.seen.lock().unwrap().len(), 1);
    assert_eq!(
        s.call("list_pacticipants", json!({})).await["data"]["items"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(s.seen.lock().unwrap().len(), 2);
}
#[tokio::test]
async fn mcp_cursor_replay_expiry_cross_tool_and_restart() {
    let mut s = Session::new(200, participants(3)).await;
    let first = s.call("list_pacticipants", json!({"pageSize":1})).await;
    assert!(
        first.get("data").is_some(),
        "expected successful frozen page: {first}"
    );
    let cursor = first["data"]["nextCursor"].as_str().unwrap();
    assert_eq!(uuid::Uuid::parse_str(cursor).unwrap().get_version_num(), 4);
    code(
        &s.call("list_integrations", json!({"cursor":cursor})).await,
        "CURSOR_INVALID",
    );
    code(
        &s.call("list_pacticipants", json!({"cursor":"malformed"}))
            .await,
        "CURSOR_INVALID",
    );
    code(
        &s.call("list_pacticipants", json!({"cursor":""})).await,
        "INVALID_INPUT",
    );
    code(
        &s.call(
            "list_pacticipants",
            json!({"cursor":uuid::Uuid::new_v4().to_string()}),
        )
        .await,
        "CURSOR_EXPIRED",
    );
    let mixed = s
        .call(
            "list_pacticipants",
            json!({"cursor":cursor,"pageSize":"bad"}),
        )
        .await;
    assert_eq!(
        mixed["error"]["details"],
        json!({"reasonCode":"SCHEMA_VALIDATION","field":"/cursor"})
    );
    let second = s.call("list_pacticipants", json!({"cursor":cursor})).await;
    assert_eq!(second["data"]["items"][0]["name"], "P1");
    assert_eq!(
        s.call("list_pacticipants", json!({"cursor":cursor})).await,
        second
    );
    let mut restarted = Session::new(200, participants(3)).await;
    code(
        &restarted
            .call("list_pacticipants", json!({"cursor":cursor}))
            .await,
        "CURSOR_EXPIRED",
    );
    assert_eq!(s.seen.lock().unwrap().len(), 1);
    assert!(restarted.seen.lock().unwrap().is_empty());
}
#[tokio::test]
async fn mcp_cursor_interleaved_eviction_is_explicit() {
    let mut s = Session::new(200, participants(3)).await;
    let mut cursors = vec![];
    for _ in 0..5 {
        let first = s.call("list_pacticipants", json!({"pageSize":1})).await;
        assert!(
            first.get("data").is_some(),
            "expected successful page: {first}"
        );
        cursors.push(first["data"]["nextCursor"].as_str().unwrap().to_owned());
    }
    code(
        &s.call("list_pacticipants", json!({"cursor":cursors[0]}))
            .await,
        "CURSOR_EXPIRED",
    );
    assert_eq!(
        s.call("list_pacticipants", json!({"cursor":cursors[1]}))
            .await["data"]["items"][0]["name"],
        "P1"
    );
    assert_eq!(s.seen.lock().unwrap().len(), 5);
}
#[tokio::test]
async fn mcp_size_caps_never_truncate_decisions() {
    let mut s = Session::new(200, json!({"plugin":"\\\"".repeat(180_000)})).await;
    let frame=s.rpc("tools/call",json!({"name":"pactling_get_pact","arguments":{"consumer":"C","provider":"P","consumerVersion":"1"}})).await;
    code(&frame["result"]["structuredContent"], "RESPONSE_TOO_LARGE");
    assert!(serde_json::to_vec(&frame["result"]).unwrap().len() <= 1024 * 1024);
    s.reply(
        200,
        json!({"_embedded":{"pacticipants":[{"name":"A".repeat(300_000)}]}}),
    );
    code(
        &s.call("list_pacticipants", json!({})).await,
        "RESPONSE_TOO_LARGE",
    );
}
#[tokio::test]
async fn mcp_page_shrinks_to_longest_fitting_prefix_and_replays_tokens() {
    let records = (0..5)
        .map(|n| json!({"name":format!("{n}{}","x".repeat(100_000))}))
        .collect::<Vec<_>>();
    let mut s = Session::new(200, json!({"_embedded":{"pacticipants":records}})).await;
    let first = s.call("list_pacticipants", json!({"pageSize":5})).await;
    assert!(
        first.get("data").is_some(),
        "expected successful page: {first}"
    );
    assert_eq!(first["data"]["items"].as_array().unwrap().len(), 2);
    assert!(
        first.get("data").is_some(),
        "expected successful frozen page: {first}"
    );
    let cursor = first["data"]["nextCursor"].as_str().unwrap();
    let second = s.call("list_pacticipants", json!({"cursor":cursor})).await;
    assert_eq!(second["data"]["items"].as_array().unwrap().len(), 2);
    assert_eq!(
        second,
        s.call("list_pacticipants", json!({"cursor":cursor})).await
    );
    let last = s
        .call(
            "list_pacticipants",
            json!({"cursor":second["data"]["nextCursor"]}),
        )
        .await;
    assert_eq!(last["data"]["items"].as_array().unwrap().len(), 1);
    assert!(last["data"]["nextCursor"].is_null());
}
#[tokio::test]
async fn mcp_cancellation_releases_resources() {
    use tokio::io::AsyncReadExt;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let (closed_tx, closed_rx) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        while !request.ends_with(b"\r\n\r\n") {
            request.push(socket.read_u8().await.unwrap());
        }
        socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 100000\r\nContent-Type: application/json\r\n\r\n{").await.unwrap();
        started_tx.send(()).unwrap();
        let mut byte = [0];
        let closed = matches!(socket.read(&mut byte).await, Ok(0));
        let _ = closed_tx.send(closed);
    });
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
    let mut input = child.stdin.take().unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap()).lines();
    input.write_all(format!("{}\n",json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"cancel","version":"1"}}})).as_bytes()).await.unwrap();
    timeout(Duration::from_secs(5), output.next_line())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    for frame in [
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"pactling_list_pacticipants","arguments":{}}}),
    ] {
        input
            .write_all(format!("{frame}\n").as_bytes())
            .await
            .unwrap();
    }
    timeout(Duration::from_secs(5), started_rx)
        .await
        .unwrap()
        .unwrap();
    input.write_all(format!("{}\n",json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":3,"reason":"test"}})).as_bytes()).await.unwrap();
    let released = timeout(Duration::from_secs(2), closed_rx).await;
    assert!(
        matches!(released, Ok(Ok(true))),
        "cancellation must drop in-flight HTTP response before broker timeout: {released:?}"
    );
    input
        .write_all(
            format!(
                "{}\n",
                json!({"jsonrpc":"2.0","id":5,"method":"tools/list","params":{}})
            )
            .as_bytes(),
        )
        .await
        .unwrap();
    loop {
        let line = timeout(Duration::from_secs(5), output.next_line())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let frame: Value = serde_json::from_str(&line).unwrap();
        if frame["id"] == 5 {
            assert_eq!(frame["result"]["tools"].as_array().unwrap().len(), 5);
            break;
        }
    }
    server.await.unwrap();
}
