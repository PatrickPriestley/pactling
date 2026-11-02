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

#[tokio::test]
async fn mcp_integration_catalog_default_pages_preserve_observed_order() {
    let records=(0..53).rev().map(|n|json!({"consumer":{"name":format!("C{n}")},"provider":{"name":"P"},"_links":{"pb:matrix":{"href":"https://foreign/ignored"}}})).collect::<Vec<_>>();
    let mut s = Session::new(200, json!({"_embedded":{"integrations":records}})).await;
    let mut page = s.call("list_integrations", json!({})).await;
    let mut names = vec![];
    for count in [25, 25, 3] {
        assert_eq!(page["data"]["items"].as_array().unwrap().len(), count);
        names.extend(
            page["data"]["items"]
                .as_array()
                .unwrap()
                .iter()
                .map(|x| x["consumer"].clone()),
        );
        if let Some(cursor) = page["data"]["nextCursor"].as_str() {
            code(
                &s.call("list_pacticipants", json!({"cursor":cursor})).await,
                "CURSOR_INVALID",
            );
            page = s.call("list_integrations", json!({"cursor":cursor})).await;
        }
    }
    assert_eq!(page["data"]["nextCursor"], Value::Null);
    assert_eq!(
        names,
        (0..53)
            .rev()
            .map(|n| json!(format!("C{n}")))
            .collect::<Vec<_>>()
    );
    assert_eq!(s.seen.lock().unwrap().len(), 1);
}
#[tokio::test]
async fn mcp_longest_final_page_accounts_for_smaller_null_cursor() {
    let mut s = Session::new(
        200,
        json!({"_embedded":{"pacticipants":[{"name":"A"},{"name":"B"}]}}),
    )
    .await;
    let args = json!({"name":"pactling_list_pacticipants","arguments":{"pageSize":2}});
    let baseline = s.rpc("tools/call", args.clone()).await;
    // The SDK can omit resultType for older negotiated protocols; the budget
    // deliberately measures the complete CallToolResult before that adaptation.
    let base_len = serde_json::to_vec(&rmcp::model::CallToolResult::structured(
        baseline["result"]["structuredContent"].clone(),
    ))
    .unwrap()
    .len();
    let padding = (1024 * 1024 - base_len - 20) / 4;
    s.reply(200,json!({"_embedded":{"pacticipants":[{"name":"A"},{"name":format!("B{}","x".repeat(padding))}]}}));
    let frame = s.rpc("tools/call", args).await;
    assert_eq!(frame["result"]["isError"], false);
    let result = &frame["result"];
    assert_eq!(
        result["structuredContent"]["data"]["items"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        result["structuredContent"]["data"]["nextCursor"],
        Value::Null
    );
    let length = serde_json::to_vec(result).unwrap().len();
    assert!(
        length <= 1024 * 1024 && length > 1024 * 1024 - 64,
        "boundary setup: {length}"
    );
    let mut with_token = result.clone();
    with_token["structuredContent"]["data"]["nextCursor"] = json!(uuid::Uuid::new_v4().to_string());
    with_token["content"][0]["text"] = json!(with_token["structuredContent"].to_string());
    assert!(
        serde_json::to_vec(&with_token).unwrap().len() > 1024 * 1024,
        "a hypothetical nonfinal token must exceed the cap"
    );
}
