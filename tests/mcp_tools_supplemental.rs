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
    base: String,
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
        let base = format!("http://{}/prefix", listener.local_addr().unwrap());
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
            base,
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
fn fixture(path: &str) -> Value {
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}
fn explanation() -> Value {
    json!({"pacticipant":"C","selector":{"kind":"version","version":"1"},"target":{"kind":"environment","name":"test"}})
}
fn matrix() -> Value {
    fixture("tests/fixtures/ruby-broker/2.121.2/phase4/GET_matrix_integration.json")
}
fn pair() -> Value {
    json!({"participantA":"Consumer","participantB":"Provider"})
}
fn pact() -> Value {
    json!({"consumer":"C","provider":"P","consumerVersion":"1"})
}
fn code(v: &Value, c: &str) {
    assert_eq!(v["error"]["code"], c, "{v}");
    assert_eq!(v["adapterSchemaVersion"], "1.0");
    assert_eq!(v.as_object().unwrap().len(), 2);
}

#[tokio::test]
async fn mcp_catalog_unrepresentable_names_fail_without_partial_data() {
    let mut s = Session::new(200, json!({})).await;
    for name in [".", "..", "bad\nname"] {
        for (tool, body) in [
            (
                "list_pacticipants",
                json!({"_embedded":{"pacticipants":[{"name":"Good"},{"name":name}]}}),
            ),
            (
                "list_integrations",
                json!({"_embedded":{"integrations":[{"consumer":{"name":"Good"},"provider":{"name":"P"},"_links":{"pb:matrix":{}}},{"consumer":{"name":name},"provider":{"name":"P"},"_links":{"pb:matrix":{}}}]}}),
            ),
        ] {
            s.reply(200, body);
            let result = s.call(tool, json!({})).await;
            code(&result, "INVALID_BROKER_RESPONSE");
            assert_eq!(
                result["error"]["message"],
                "Catalog contains an unsupported path name"
            );
            assert!(
                result["error"]["evidenceUrl"]
                    .as_str()
                    .unwrap()
                    .starts_with(&s.base)
            );
            assert!(result.get("data").is_none());
        }
        s.reply(200, matrix());
        let r = s
            .call("matrix", json!({"participantA":name,"participantB":"P"}))
            .await;
        assert_eq!(r["data"], matrix());
        let doc = fixture("tests/fixtures/pactling/explanations/allowed.json");
        s.reply(200, doc.clone());
        let mut args = explanation();
        args["pacticipant"] = json!(name);
        assert_eq!(s.call("explain_deployment", args).await["data"], doc);
    }
}
#[test]
fn mcp_bundled_explanation_refs_preserve_original_schema() {
    let schema = fixture("docs/api/mcp/pactling_explain_deployment.output.schema.json");
    assert_eq!(
        schema["$defs"]["explanation"],
        fixture("docs/api/deployment-explanations-v1.schema.json")
    );
    let validator = jsonschema::validator_for(&schema).unwrap();
    let mut doc = fixture("tests/fixtures/pactling/explanations/allowed.json");
    let wrap = |d| json!({"adapterSchemaVersion":"1.0","data":d,"coverage":"recorded_only","evidenceLinks":[]});
    assert!(validator.is_valid(&wrap(doc.clone())));
    doc["resolution"]["candidates"][0]["sources"][0]["kind"] = json!("unsupported");
    assert!(!validator.is_valid(&wrap(doc)));
}
