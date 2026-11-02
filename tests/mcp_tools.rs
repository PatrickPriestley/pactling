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
async fn mcp_schema_matches_tools_list() {
    let mut s = Session::new(200, json!({})).await;
    let r = s.rpc("tools/list", json!({})).await;
    for tool in r["result"]["tools"].as_array().unwrap() {
        let name = tool["name"].as_str().unwrap();
        for (field, suffix) in [("inputSchema", "input"), ("outputSchema", "output")] {
            assert!(tool[field].is_object(), "missing {field}");
            let schema = fixture(&format!("docs/api/mcp/{name}.{suffix}.schema.json"));
            assert_eq!(tool[field], schema);
            jsonschema::validator_for(&schema).unwrap();
        }
    }
}
#[tokio::test]
async fn mcp_tools_advertise_read_only_annotations() {
    let mut s = Session::new(200, json!({})).await;
    let r = s.rpc("tools/list", json!({})).await;
    let tools = r["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 5);
    for t in tools {
        assert_eq!(
            t["annotations"],
            json!({"readOnlyHint":true,"destructiveHint":false,"idempotentHint":true,"openWorldHint":false})
        );
    }
}
#[tokio::test]
async fn mcp_tool_descriptions_mark_untrusted_content() {
    let mut s = Session::new(200, json!({})).await;
    let r = s.rpc("tools/list", json!({})).await;
    for t in r["result"]["tools"].as_array().unwrap() {
        assert!(
            t["description"]
                .as_str()
                .unwrap()
                .contains("untrusted data"),
            "{t}"
        );
    }
}
#[tokio::test]
async fn mcp_rejects_unknown_arguments_before_http() {
    let mut s = Session::new(200, json!({})).await;
    for (tool, arg) in [
        (
            "get_pact",
            json!({"consumer":"C","provider":"P","consumerVersion":"1","url":"SECRET"}),
        ),
        (
            "matrix",
            json!({"participantA":"A","participantB":"B","method":"DELETE"}),
        ),
        ("list_integrations", json!({"headers":{}})),
        ("list_pacticipants", json!({"pageSize":0})),
        (
            "explain_deployment",
            json!({"pacticipant":"C","selector":{"kind":"latest","tag":""},"target":{"kind":"environment","name":"t"}}),
        ),
    ] {
        let r = s.call(tool, arg).await;
        code(&r, "INVALID_INPUT");
        assert_eq!(r["error"]["details"]["reasonCode"], "SCHEMA_VALIDATION");
        assert!(!r.to_string().contains("SECRET"));
    }
    assert!(s.seen.lock().unwrap().is_empty());
}
#[tokio::test]
async fn mcp_rejects_extra_main_branch_fields() {
    let mut s = Session::new(200, json!({})).await;
    let mut args = explanation();
    args["selector"] = json!({"kind":"mainBranch","version":"1"});
    let r = s.call("explain_deployment", args).await;
    code(&r, "INVALID_INPUT");
    assert_eq!(r["error"]["details"]["field"], "/selector");
    assert!(s.seen.lock().unwrap().is_empty());
}
#[tokio::test]
async fn mcp_recursive_input_limits_and_union_validation() {
    let mut s = Session::new(200, json!({})).await;
    for (field, value) in [
        ("selector", json!({"kind":"version"})),
        ("selector", json!({"kind":"branch","branch":"x","tag":"y"})),
        (
            "target",
            json!({"kind":"environment","name":"t","url":"SECRET"}),
        ),
        ("ignores", json!([{"pacticipant":"P","unexpected":true}])),
        ("ignores", json!(vec![json!({"pacticipant":"P"}); 101])),
        ("pacticipant", json!("x".repeat(1025))),
        ("pacticipant", json!(null)),
    ] {
        let mut a = explanation();
        a[field] = value;
        let r = s.call("explain_deployment", a).await;
        code(&r, "INVALID_INPUT");
        assert_eq!(r["error"]["details"]["field"], format!("/{field}"));
    }
    for a in [
        json!({"pageSize":101}),
        json!({"pageSize":1.5}),
        json!({"cursor":7,"pageSize":"wrong"}),
    ] {
        let r = s.call("list_integrations", a.clone()).await;
        code(&r, "INVALID_INPUT");
        if a.get("cursor").is_some() {
            assert_eq!(r["error"]["details"]["field"], "/cursor");
        }
    }
    assert!(s.seen.lock().unwrap().is_empty());
}
#[tokio::test]
async fn mcp_error_codes_are_stable() {
    let mut s = Session::new(401, json!("SECRET")).await;
    for (status, body, expected, retry) in [
        (401, json!("SECRET"), "AUTH_REQUIRED", false),
        (403, json!({"secret":"SECRET"}), "FORBIDDEN", false),
        (400, json!({}), "INVALID_INPUT", false),
        (422, json!({}), "INVALID_INPUT", false),
        (429, json!({}), "RATE_LIMITED", true),
        (500, json!({}), "BROKER_UNAVAILABLE", true),
        (503, json!({}), "BROKER_UNAVAILABLE", true),
        (302, json!({}), "BROKER_HTTP_ERROR", false),
        (414, json!({}), "BROKER_HTTP_ERROR", false),
        (431, json!({}), "BROKER_HTTP_ERROR", false),
        (
            404,
            json!({"error":"The requested document was not found on this server."}),
            "NOT_FOUND",
            false,
        ),
        (
            404,
            json!({"error":"No pacticipant with name 'X' found"}),
            "NOT_FOUND",
            false,
        ),
        (404, json!({"proxy":"SECRET"}), "BROKER_HTTP_ERROR", false),
        (418, json!({}), "BROKER_HTTP_ERROR", false),
    ] {
        s.reply(status, body);
        let r = s.call("get_pact", pact()).await;
        code(&r, expected);
        assert_eq!(r["error"]["httpStatus"], status);
        assert_eq!(r["error"]["retryable"], retry);
        assert!(!r.to_string().contains("SECRET"));
        if status == 302 {
            assert_eq!(
                r["error"]["message"],
                "Configured broker base URL redirected"
            );
        }
        if status == 414 || status == 431 {
            assert_eq!(
                r["error"]["message"],
                "Broker or proxy rejected the request size; reduce selectors or ignores"
            );
        }
    }
}
#[tokio::test]
async fn mcp_missing_explanation_route_is_capability_error() {
    let mut s = Session::new(404, json!({})).await;
    let r = s.call("explain_deployment", explanation()).await;
    code(&r, "BROKER_CAPABILITY_UNAVAILABLE");
    assert_eq!(
        r["error"]["message"],
        "Configured broker does not expose the pactling explanation route"
    );
    assert_eq!(s.seen.lock().unwrap().len(), 1);
}
#[tokio::test]
async fn mcp_unknown_environment_detail_fallback() {
    let mut s = Session::new(400, json!({})).await;
    for (body, reason, field) in [
        (
            json!({"schemaVersion":"1.0","error":{"code":"INVALID_QUERY","details":["Environment with name 'test' does not exist"]}}),
            "ENVIRONMENT_NOT_FOUND",
            json!("/target/name"),
        ),
        (
            json!({"schemaVersion":"1.0","error":{"code":"INVALID_QUERY","details":["Environment with name 'other' does not exist"]}}),
            "BROKER_REJECTED_QUERY",
            Value::Null,
        ),
        (
            json!({"schemaVersion":"1.0","error":{"code":"INVALID_QUERY","details":["Environment with name 'test' does not exist","SECRET"]}}),
            "BROKER_REJECTED_QUERY",
            Value::Null,
        ),
    ] {
        s.reply(400, body);
        let r = s.call("explain_deployment", explanation()).await;
        code(&r, "INVALID_INPUT");
        assert_eq!(r["error"]["details"]["reasonCode"], reason);
        assert_eq!(r["error"]["details"]["field"], field);
        assert!(!r.to_string().contains("SECRET"));
    }
}
#[tokio::test]
async fn mcp_exact_pact_preserves_plugin_fields() {
    let doc = json!({"consumer":{"name":"C"},"provider":{"name":"P"},"interactions":[{"type":"Synchronous/Messages","pluginConfiguration":{"grpc":{"descriptorKey":"z"}},"contents":{"content":"AA==","encoded":"base64"}}],"metadata":{"pactSpecification":{"version":"4.0"},"plugins":[{"name":"protobuf","version":"9"}]},"future":{"arbitrary":[null,true,9]},"_links":{"write":{"href":"https://foreign.invalid/write"}}});
    let mut s = Session::new(200, doc.clone()).await;
    let r = s.call("get_pact", pact()).await;
    assert_eq!(r["data"], doc);
    assert_eq!(r["coverage"], "recorded_only");
    assert_eq!(
        r["evidenceLinks"],
        json!([{"rel":"source","href":format!("{}/pacts/provider/P/consumer/C/version/1",s.base)}])
    );
}
#[tokio::test]
async fn mcp_pair_matrix_preserves_both_directions() {
    let mut doc = matrix();
    let mut reverse = doc["matrix"][0].clone();
    reverse
        .as_object_mut()
        .unwrap()
        .insert("consumer".into(), doc["matrix"][0]["provider"].clone());
    reverse
        .as_object_mut()
        .unwrap()
        .insert("provider".into(), doc["matrix"][0]["consumer"].clone());
    doc["matrix"].as_array_mut().unwrap().push(reverse);
    let mut s = Session::new(200, doc.clone()).await;
    assert_eq!(s.call("matrix", pair()).await["data"], doc);
    assert_eq!(
        s.seen.lock().unwrap()[0],
        "/prefix/matrix?q%5B%5D%5Bpacticipant%5D=Consumer&q%5B%5D%5Bpacticipant%5D=Provider"
    );
}
#[tokio::test]
async fn mcp_matrix_grouping_preserves_broker_document() {
    let doc = matrix();
    let mut s = Session::new(200, doc.clone()).await;
    for group in ["cvpv", "cvp", "cp"] {
        let mut a = pair();
        a["latestby"] = json!(group);
        assert_eq!(s.call("matrix", a).await["data"], doc);
        assert!(
            s.seen
                .lock()
                .unwrap()
                .last()
                .unwrap()
                .ends_with(&format!("&latestby={group}"))
        );
    }
}
#[tokio::test]
async fn mcp_pair_same_name_and_unknown_match_broker() {
    let mut s = Session::new(200, matrix()).await;
    let r = s
        .call("matrix", json!({"participantA":"A","participantB":"A"}))
        .await;
    assert_eq!(r["data"], matrix());
    s.reply(400, json!({"errors":["Pacticipant Unknown not found"]}));
    let r = s.call("matrix", pair()).await;
    code(&r, "INVALID_INPUT");
    assert_eq!(r["error"]["details"]["reasonCode"], "BROKER_REJECTED_QUERY");
}
#[tokio::test]
async fn mcp_explanation_keeps_v1_document() {
    let mut s = Session::new(200, json!({})).await;
    for state in ["allowed", "blocked", "unknown"] {
        let mut doc = fixture(&format!(
            "tests/fixtures/pactling/explanations/{state}.json"
        ));
        doc["futureField"] = json!({"preserved":true});
        s.reply(200, doc.clone());
        let r = s.call("explain_deployment", explanation()).await;
        assert_eq!(r["data"], doc);
        let schema = fixture("docs/api/mcp/pactling_explain_deployment.output.schema.json");
        assert!(jsonschema::is_valid(&schema, &r));
    }
}
#[tokio::test]
async fn mcp_explanation_query_matches_cli_fixture() {
    let doc = fixture("tests/fixtures/pactling/explanations/allowed.json");
    let mut s = Session::new(200, doc).await;
    for (selector, keys) in [
        (
            json!({"kind":"version","version":"v /?"}),
            "q%5B%5D%5Bversion%5D=v+%2F%3F",
        ),
        (json!({"kind":"latest"}), "q%5B%5D%5Blatest%5D=true"),
        (
            json!({"kind":"latest","tag":"dev"}),
            "q%5B%5D%5Blatest%5D=true&q%5B%5D%5Btag%5D=dev",
        ),
        (
            json!({"kind":"branch","branch":"main"}),
            "q%5B%5D%5Blatest%5D=true&q%5B%5D%5Bbranch%5D=main",
        ),
        (
            json!({"kind":"mainBranch"}),
            "q%5B%5D%5Blatest%5D=true&q%5B%5D%5BmainBranch%5D=true",
        ),
    ] {
        for (target, target_keys) in [
            (
                json!({"kind":"environment","name":"test"}),
                "environment=test",
            ),
            (json!({"kind":"tag","name":"prod"}), "latest=true&tag=prod"),
        ] {
            let mut a = explanation();
            a["selector"] = selector.clone();
            a["target"] = target;
            a["ignores"] = json!([{"pacticipant":"P"},{"pacticipant":"Q","version":"2"}]);
            assert!(s.call("explain_deployment", a).await.get("data").is_some());
            assert_eq!(
                s.seen.lock().unwrap().last().unwrap(),
                &format!(
                    "/prefix/pactling/v1/can-i-deploy/explanation?q%5B%5D%5Bpacticipant%5D=C&{keys}&latestby=cvp&{target_keys}&ignore%5B%5D%5Bpacticipant%5D=P&ignore%5B%5D%5Bpacticipant%5D=Q&ignore%5B%5D%5Bversion%5D=2"
                )
            );
        }
    }
}
#[tokio::test]
async fn mcp_missing_selector_preserves_broker_status_and_decision() {
    let fixtures = fixture("docs/probes/mcp-adapter/missing-selectors.json");
    let mut s = Session::new(200, json!({})).await;
    for record in fixtures
        .as_array()
        .unwrap()
        .iter()
        .filter(|v| v["path"].as_str().unwrap().contains("/explanation?"))
    {
        let status = record["status"].as_u64().unwrap() as u16;
        s.reply(status, record["document"].clone());
        let r = s.call("explain_deployment", explanation()).await;
        if status == 400 {
            code(&r, "INVALID_INPUT");
            assert_eq!(r["error"]["httpStatus"], 400);
        } else {
            assert_eq!(r["data"], record["document"]);
            assert_eq!(r["data"]["decision"], "blocked");
        }
    }
}
#[tokio::test]
async fn mcp_malformed_responses_and_contradictory_verdicts() {
    let mut s = Session::new(200, json!({})).await;
    for (name, args, doc) in [
        ("get_pact", pact(), json!([])),
        ("matrix", pair(), json!({"matrix":[],"summary":{}})),
        (
            "list_integrations",
            json!({}),
            json!({"_embedded":{"integrations":[{"consumer":{"name":"C"}}]}}),
        ),
        ("list_pacticipants", json!({}), json!({})),
        (
            "explain_deployment",
            explanation(),
            json!({"schemaVersion":"1.0","error":{"code":"INVALID_QUERY","details":[]}}),
        ),
    ] {
        s.reply(200, doc);
        code(&s.call(name, args).await, "INVALID_BROKER_RESPONSE");
    }
    for (path, value) in [
        ("schemaVersion", json!("2.0")),
        (
            "compatibility",
            json!({"deployable":false,"success":1,"failed":0,"unknown":0,"ignored":0,"hasIgnores":false}),
        ),
        ("rows", json!([{}])),
        ("evaluation", json!({})),
    ] {
        let mut d = fixture("tests/fixtures/pactling/explanations/allowed.json");
        d[path] = value;
        s.reply(200, d);
        code(
            &s.call("explain_deployment", explanation()).await,
            "INVALID_BROKER_RESPONSE",
        );
    }
    *s.response.lock().unwrap() = (200, "<html>SECRET</html>".into());
    code(&s.call("get_pact", pact()).await, "INVALID_BROKER_RESPONSE");
}
#[tokio::test]
async fn mcp_pacticipant_latest_version_absent_and_malformed() {
    let mut s=Session::new(200,json!({"_embedded":{"pacticipants":[{"name":"NoVersion"},{"name":"C","_embedded":{"latestVersion":{"number":"1"}}}]}})).await;
    let r = s.call("list_pacticipants", json!({})).await;
    assert_eq!(r["data"]["items"][0]["latestVersion"], Value::Null);
    assert_eq!(r["data"]["items"][1]["latestVersion"], "1");
    for stub in [json!(null), json!({}), json!({"number":1})] {
        s.reply(
            200,
            json!({"_embedded":{"pacticipants":[{"name":"C","_embedded":{"latestVersion":stub}}]}}),
        );
        code(
            &s.call("list_pacticipants", json!({})).await,
            "INVALID_BROKER_RESPONSE",
        );
    }
}
#[tokio::test]
async fn mcp_catalog_links_use_configured_origin() {
    let mut s=Session::new(200,json!({"_embedded":{"integrations":[{"consumer":{"name":"C /?"},"provider":{"name":"P#"},"_links":{"pb:matrix":{"href":"https://evil.invalid/write"},"pb:delete_integration":{"href":"https://evil.invalid/delete"},"pb:dashboard":{"href":"https://evil.invalid/d"}}}]}})).await;
    let r = s.call("list_integrations", json!({})).await;
    assert_eq!(r["data"]["items"][0]["consumer"], "C /?");
    let links = r["data"]["items"][0]["evidenceLinks"].as_array().unwrap();
    assert!(
        links
            .iter()
            .any(|l| l["href"] == format!("{}/matrix/provider/P%23/consumer/C%20%2F%3F", s.base))
    );
    assert!(
        links
            .iter()
            .all(|l| l["href"].as_str().unwrap().starts_with(&s.base))
    );
    assert!(!r.to_string().contains("evil.invalid"));
    assert!(!r.to_string().contains("delete"));
    assert_eq!(r["data"].as_object().unwrap().len(), 3);
    assert!(r["data"]["retrievedAt"].is_string());
    assert_eq!(r["data"]["nextCursor"], Value::Null);
    s.reply(200,json!({"_embedded":{"pacticipants":[{"name":"C /?","_links":{"self":{"href":"https://evil.invalid/write"}}}]}}));
    let r = s.call("list_pacticipants", json!({})).await;
    assert_eq!(
        r["data"]["items"][0]["href"],
        format!("{}/pacticipants/C%20%2F%3F", s.base)
    );
    assert!(!r.to_string().contains("evil.invalid"));
    assert_eq!(s.seen.lock().unwrap().len(), 2);
}
#[tokio::test]
async fn mcp_oversize_guidance_depends_on_grouping() {
    let mut doc = matrix();
    doc["padding"] = json!("x".repeat(4 * 1024 * 1024));
    let mut s = Session::new(200, doc).await;
    for group in [None, Some("cp"), Some("cvp"), Some("cvpv")] {
        let mut a = pair();
        if let Some(g) = group {
            a["latestby"] = json!(g);
        }
        let r = s.call("matrix", a).await;
        code(&r, "RESPONSE_TOO_LARGE");
        assert_eq!(
            r["error"]["message"],
            if group.is_none() {
                "Retry with latestby=cp to request the broker's grouped matrix"
            } else {
                "Open the source in an authorized broker client; this adapter cannot return the complete document within its size limit"
            }
        );
        assert!(
            r["error"]["evidenceUrl"]
                .as_str()
                .unwrap()
                .starts_with(&s.base)
        );
        assert!(r.get("data").is_none());
    }
}
