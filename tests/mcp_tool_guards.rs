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
async fn mcp_guards_reject_false_not_found_and_environment_matches() {
    let mut s = Session::new(404, json!({})).await;
    for body in [
        json!({"error":"proxy ' found"}),
        json!({"error":"No pacticipant with name 'X' guessed"}),
        json!({"error":5}),
    ] {
        s.reply(404, body);
        code(&s.call("get_pact", pact()).await, "BROKER_HTTP_ERROR");
    }
    for (version, error_code) in [("2.0", "INVALID_QUERY"), ("1.0", "OTHER")] {
        s.reply(400,json!({"schemaVersion":version,"error":{"code":error_code,"details":["Environment with name 'test' does not exist"]}}));
        let r = s.call("explain_deployment", explanation()).await;
        assert_eq!(
            r["error"]["details"],
            json!({"reasonCode":"BROKER_REJECTED_QUERY"})
        );
    }
    s.reply(400,json!({"schemaVersion":"1.0","error":{"code":"INVALID_QUERY","details":["Environment with name 'test' does not exist"]}}));
    let mut a = explanation();
    a["target"] = json!({"kind":"tag","name":"test"});
    assert_eq!(
        s.call("explain_deployment", a).await["error"]["details"],
        json!({"reasonCode":"BROKER_REJECTED_QUERY"})
    );
    assert_eq!(
        s.call("matrix", pair()).await["error"]["details"],
        json!({"reasonCode":"BROKER_REJECTED_QUERY"})
    );
}
#[tokio::test]
async fn mcp_guards_catalog_relation_presence_and_complete_pages() {
    let mut s = Session::new(
        200,
        json!({"_embedded":{"integrations":[{"consumer":{"name":"C"},"provider":{"name":"P"}}]}}),
    )
    .await;
    assert_eq!(
        s.call("list_integrations", json!({})).await["data"]["items"][0]["evidenceLinks"],
        json!([])
    );
    s.reply(
        200,
        json!({"_embedded":{"pacticipants":[{"name":"一"},{"name":"é"}]}}),
    );
    let r = s.call("list_pacticipants", json!({"pageSize":2})).await;
    assert_eq!(r["data"]["items"].as_array().unwrap().len(), 2);
    assert_eq!(
        r["data"]["items"][1]["href"],
        format!("{}/pacticipants/%C3%A9", s.base)
    );
    s.reply(200,json!({"_embedded":{"integrations":[{"consumer":{"name":"一"},"provider":{"name":"é"},"_links":{"pb:dashboard":{}}}]}}));
    let r = s.call("list_integrations", json!({})).await;
    assert_eq!(
        r["data"]["items"][0]["evidenceLinks"],
        json!([{"rel":"dashboard","href":format!("{}/dashboard/provider/%C3%A9/consumer/%E4%B8%80",s.base)}])
    );
}
#[tokio::test]
async fn mcp_guards_transport_path_and_unknown_tool_errors() {
    let mut s = Session::new(200, json!({})).await;
    for field in ["consumer", "provider", "consumerVersion"] {
        for name in [".", "..", "C\n"] {
            let mut a = pact();
            a[field] = json!(name);
            let r = s.call("get_pact", a).await;
            code(&r, "INVALID_INPUT");
            assert_eq!(
                r["error"]["details"],
                json!({"reasonCode":"SCHEMA_VALIDATION","field":format!("/{field}")})
            );
        }
    }
    // Several unrepresentable segments: the first in argument order is named.
    let mut a = pact();
    a["provider"] = json!("..");
    a["consumerVersion"] = json!(".");
    let r = s.call("get_pact", a.clone()).await;
    assert_eq!(r["error"]["details"]["field"], "/provider");
    a["consumer"] = json!(".");
    let r = s.call("get_pact", a).await;
    assert_eq!(r["error"]["details"]["field"], "/consumer");
    let r = s
        .rpc(
            "tools/call",
            json!({"name":"unknown","arguments":explanation()}),
        )
        .await;
    assert!(r["error"].is_object());
    assert!(s.seen.lock().unwrap().is_empty());
    s.server.abort();
    tokio::task::yield_now().await;
    let r = s.call("get_pact", pact()).await;
    code(&r, "BROKER_UNAVAILABLE");
    assert_eq!(r["error"]["retryable"], true);
    assert!(!r.to_string().contains(&s.base));
}
#[tokio::test]
async fn mcp_guards_input_field_limits_and_safe_pointers() {
    let mut s = Session::new(200, json!({})).await;
    for (tool, mut args, fields) in [
        (
            "get_pact",
            pact(),
            vec!["consumer", "provider", "consumerVersion"],
        ),
        ("matrix", pair(), vec!["participantA", "participantB"]),
        ("explain_deployment", explanation(), vec!["pacticipant"]),
    ] {
        for field in fields {
            let original = args[field].clone();
            for value in [json!(""), json!("x".repeat(1025)), json!(7)] {
                args[field] = value;
                let r = s.call(tool, args.clone()).await;
                code(&r, "INVALID_INPUT");
                assert_eq!(r["error"]["details"]["field"], format!("/{field}"));
            }
            args[field] = original;
        }
    }
    for (field, value) in [
        ("selector", json!({"kind":"new"})),
        ("selector", json!({"kind":"latest","tag":null})),
        ("selector", json!({"kind":"version","version":""})),
        ("selector", json!({"kind":"branch","branch":""})),
        ("target", json!({"kind":"tag","name":""})),
        ("target", json!({"kind":"other","name":"x"})),
        ("ignores", json!([{"pacticipant":"P","version":""}])),
        ("ignores", json!([{"pacticipant":""}])),
        ("ignores", json!({})),
    ] {
        let mut args = explanation();
        args[field] = value;
        code(&s.call("explain_deployment", args).await, "INVALID_INPUT");
    }
    for (tool, args) in [
        (
            "matrix",
            json!({"participantA":"A","participantB":"B","latestby":"other"}),
        ),
        ("get_pact", json!({"consumer":"A"})),
        ("list_pacticipants", json!({"pageSize":null})),
        ("list_integrations", json!({"cursor":""})),
        ("list_integrations", json!({"cursor":7})),
    ] {
        code(&s.call(tool, args).await, "INVALID_INPUT");
    }
    assert!(s.seen.lock().unwrap().is_empty());
}
#[tokio::test]
async fn mcp_guards_schema_boundary_values_remain_accepted() {
    let mut s = Session::new(200, json!({})).await;
    let name = "x".repeat(1024);
    let r = s
        .call(
            "get_pact",
            json!({"consumer":name,"provider":"P","consumerVersion":"1"}),
        )
        .await;
    assert_eq!(r["data"], json!({}));
    s.reply(200, matrix());
    assert_eq!(s.call("matrix", pair()).await["data"], matrix());
    let doc = fixture("tests/fixtures/pactling/explanations/allowed.json");
    s.reply(200, doc.clone());
    let mut args = explanation();
    args["ignores"] = json!(vec![json!({"pacticipant":"P"}); 100]);
    assert_eq!(s.call("explain_deployment", args).await["data"], doc);
    for tool in ["list_integrations", "list_pacticipants"] {
        let key = if tool == "list_integrations" {
            "integrations"
        } else {
            "pacticipants"
        };
        let mut collections = serde_json::Map::new();
        collections.insert(key.into(), json!([]));
        s.reply(200, json!({"_embedded":collections}));
        for size in [1, 100] {
            assert_eq!(
                s.call(tool, json!({"pageSize":size})).await["data"]["items"],
                json!([])
            );
        }
    }
}

#[tokio::test]
async fn mcp_preserves_numeric_v1_minor_documents_and_rejects_other_versions() {
    let mut s = Session::new(200, json!({})).await;
    for version in ["1.0", "1.9", "1.100"] {
        let mut doc = fixture("tests/fixtures/pactling/explanations/allowed.json");
        doc["schemaVersion"] = json!(version);
        s.reply(200, doc.clone());
        assert_eq!(
            s.call("explain_deployment", explanation()).await["data"],
            doc
        );
    }
    for version in ["2.0", "1.x", "1.9junk", "1.9.1", ""] {
        let mut doc = fixture("tests/fixtures/pactling/explanations/allowed.json");
        doc["schemaVersion"] = json!(version);
        s.reply(200, doc);
        code(
            &s.call("explain_deployment", explanation()).await,
            "INVALID_BROKER_RESPONSE",
        );
    }
}

#[tokio::test]
async fn mcp_v1_minor_environment_error_remains_recognized() {
    let mut s=Session::new(400,json!({"schemaVersion":"1.9","error":{"code":"INVALID_QUERY","details":["Environment with name 'test' does not exist"]}})).await;
    assert_eq!(
        s.call("explain_deployment", explanation()).await["error"]["details"],
        json!({"reasonCode":"ENVIRONMENT_NOT_FOUND","field":"/target/name"})
    );
}
