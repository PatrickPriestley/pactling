//! The actual executable serves HTTP with no subcommand and consumes its own API.
use serde_json::{Value, json};
use std::{
    net::TcpListener,
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex},
};
use tokio::process::Command as AsyncCommand;
const BIN: &str = env!("CARGO_BIN_EXE_pactling");

fn clean(command: &mut Command) {
    for (name, _) in std::env::vars_os() {
        let name_str = name.to_string_lossy();
        if name_str.starts_with("PACT_BROKER_") || name_str.starts_with("PACTLING_") {
            command.env_remove(name);
        }
    }
}
fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}
struct Server {
    child: Child,
    base: String,
    client: reqwest::Client,
}
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
impl Server {
    async fn start(auth: bool, environment_only: bool) -> Self {
        let port = free_port();
        let mut command = Command::new(BIN);
        clean(&mut command);
        command.stdout(Stdio::null()).stderr(Stdio::null());
        if environment_only {
            command
                .env("PACT_BROKER_PORT", port.to_string())
                .env("PACTLING_DATABASE_URL", "sqlite::memory:");
        } else {
            command.args([
                "--port",
                &port.to_string(),
                "--database-url",
                "sqlite::memory:",
            ]);
        }
        let mut headers = reqwest::header::HeaderMap::new();
        if auth {
            command
                .env("PACT_BROKER_BASIC_AUTH_USERNAME", "writer")
                .env("PACT_BROKER_BASIC_AUTH_PASSWORD", "disposable-write")
                .env("PACT_BROKER_BASIC_AUTH_READ_ONLY_USERNAME", "reader")
                .env(
                    "PACT_BROKER_BASIC_AUTH_READ_ONLY_PASSWORD",
                    "disposable-read",
                )
                .env("PACTLING_BEARER_TOKEN", "disposable-token");
            use base64::Engine;
            headers.insert(
                "authorization",
                format!(
                    "Basic {}",
                    base64::engine::general_purpose::STANDARD.encode("writer:disposable-write")
                )
                .parse()
                .unwrap(),
            );
        }
        let client = reqwest::Client::builder()
            .default_headers(headers)
            .build()
            .unwrap();
        let mut server = Self {
            child: command.spawn().unwrap(),
            base: format!("http://127.0.0.1:{port}"),
            client,
        };
        for _ in 0..100 {
            if let Ok(response) = server.client.get(&server.base).send().await {
                assert_eq!(
                    response.status(),
                    200,
                    "no-subcommand server must answer its index"
                );
                return server;
            }
            assert!(
                server.child.try_wait().unwrap().is_none(),
                "server exited before listening"
            );
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
        panic!("server did not become ready")
    }
    async fn put(&self, path: &str, value: Value) -> Value {
        self.client
            .put(format!("{}{path}", self.base))
            .json(&value)
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .await
            .unwrap()
    }
    async fn post(&self, path: &str, value: Value) -> Value {
        let url = if path.starts_with("http") {
            path.to_string()
        } else {
            format!("{}{path}", self.base)
        };
        self.client
            .post(url)
            .json(&value)
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .await
            .unwrap()
    }
    async fn seed(&self, success: Option<bool>) {
        let pact = self
            .put(
                "/pacts/provider/P/consumer/C/version/1",
                json!({"consumer":{"name":"C"},"provider":{"name":"P"},"interactions":[]}),
            )
            .await;
        self.put("/pacticipants/C/branches/main/versions/1", json!({}))
            .await;
        self.client
            .put(format!("{}/pacticipants/C/versions/1/tags/dev", self.base))
            .json(&json!({}))
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap();
        self.put("/pacticipants/P/versions/2", json!({})).await;
        self.client
            .put(format!("{}/pacticipants/P/versions/2/tags/dev", self.base))
            .json(&json!({}))
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap();
        let env = self
            .post("/environments", json!({"name":"test","production":false}))
            .await;
        self.post(
            &format!(
                "/pacticipants/P/versions/2/deployed-versions/environment/{}",
                env["uuid"].as_str().unwrap()
            ),
            json!({}),
        )
        .await;
        if let Some(success) = success {
            self.post(
                pact["_links"]["pb:publish-verification-results"]["href"]
                    .as_str()
                    .unwrap(),
                json!({"providerApplicationVersion":"2","success":success}),
            )
            .await;
        }
    }
}
async fn explain(base: &str, args: &[&str], env: &[(&str, &str)]) -> std::process::Output {
    let mut command = Command::new(BIN);
    clean(&mut command);
    command.arg("explain");
    if !base.is_empty() {
        command.args(["--broker-base-url", base]);
    }
    command.args(args).envs(env.iter().copied());
    AsyncCommand::from(command).output().await.unwrap()
}
fn assert_code(output: &std::process::Output, code: i32) {
    assert_eq!(
        output.status.code(),
        Some(code),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
const PIN: &[&str] = &[
    "--pacticipant",
    "C",
    "--version",
    "1",
    "--to-environment",
    "test",
];

#[tokio::test]
async fn real_binary_explains_all_verdicts_and_preserves_endpoint_json_bytes() {
    for (success, decision, code) in [
        (Some(true), "allowed", 0),
        (Some(false), "blocked", 1),
        (None, "unknown", 2),
    ] {
        let server = Server::start(false, true).await;
        server.seed(success).await;
        // Capture the actual upstream response, rather than compare two evaluations'
        // request-local UUIDs and timestamps or reserialize an expected document.
        let captured = Arc::new(Mutex::new(Vec::<u8>::new()));
        let saved = captured.clone();
        let upstream = server.base.clone();
        let router = axum::Router::new().fallback(move |uri: axum::http::Uri| {
            let saved = saved.clone();
            let upstream = upstream.clone();
            async move {
                let response = reqwest::get(format!("{upstream}{uri}")).await.unwrap();
                assert_eq!(response.status(), 200);
                let bytes = response.bytes().await.unwrap();
                *saved.lock().unwrap() = bytes.to_vec();
                ([("content-type", "application/json")], bytes)
            }
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let proxy = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let mut args = PIN.to_vec();
        args.extend(["--output", "json"]);
        let output = explain(&proxy, &args, &[]).await;
        task.abort();
        assert_code(&output, code);
        assert!(output.stderr.is_empty());
        assert_eq!(
            output.stdout,
            *captured.lock().unwrap(),
            "JSON output must preserve response bytes"
        );
        let document: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(document["decision"], decision);
        assert_eq!(document["compatibility"]["deployable"], json!(success));
        let text = explain(&server.base, PIN, &[]).await;
        assert_code(&text, code);
        let text = String::from_utf8(text.stdout).unwrap();
        assert!(text.to_lowercase().contains(decision));
        for reason in document["reasons"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|r| r["role"] == "decisive")
        {
            assert!(text.contains(reason["message"].as_str().unwrap()), "{text}");
        }
        for action in document["nextActions"].as_array().unwrap() {
            assert!(text.contains(action["type"].as_str().unwrap()), "{text}");
            assert!(
                text.contains(action["target"]["contractSha"].as_str().unwrap()),
                "{text}"
            );
            assert!(text.contains("P") && text.contains("2"), "{text}");
            assert!(
                text.contains(document["rows"][0]["contract"]["href"].as_str().unwrap()),
                "action evidence must include its contract link: {text}"
            );
            if let Some(href) = action["target"]["verificationHref"].as_str() {
                assert!(text.contains(href), "{text}");
            }
        }
    }
}

#[tokio::test]
async fn real_binary_supports_unpinned_selectors_and_auth_environment() {
    let server = Server::start(true, false).await;
    server.seed(Some(true)).await;
    let basic = [
        ("PACT_BROKER_BASE_URL", server.base.as_str()),
        ("PACT_BROKER_USERNAME", "reader"),
        ("PACT_BROKER_PASSWORD", "disposable-read"),
    ];
    for selector in [
        vec!["--version", "1"],
        vec!["--latest"],
        vec!["--latest", "dev"],
        vec!["--branch", "main"],
        vec!["--main-branch"],
    ] {
        for target in [["--to-environment", "test"], ["--to", "dev"]] {
            let mut args = vec!["--pacticipant", "C"];
            args.extend(selector.iter().copied());
            args.extend(target);
            args.extend(["--output", "json"]);
            let output = explain("", &args, &basic).await;
            assert_code(&output, 0);
            assert_eq!(
                serde_json::from_slice::<Value>(&output.stdout).unwrap()["decision"],
                "allowed"
            );
        }
    }
    let token = [
        ("PACT_BROKER_BASE_URL", server.base.as_str()),
        ("PACT_BROKER_TOKEN", "disposable-token"),
    ];
    assert_code(&explain("", PIN, &token).await, 0);
    let mut basic_flags = PIN.to_vec();
    basic_flags.extend([
        "--broker-username",
        "reader",
        "--broker-password",
        "disposable-read",
    ]);
    assert_code(&explain(&server.base, &basic_flags, &[]).await, 0);
    let mut token_flags = PIN.to_vec();
    token_flags.extend(["--broker-token", "disposable-token"]);
    assert_code(&explain(&server.base, &token_flags, &[]).await, 0);
    assert_code(&explain(&server.base, PIN, &[]).await, 3);
    token_flags.pop();
    token_flags.push("bad-token");
    let denied = explain(&server.base, &token_flags, &[]).await;
    assert_code(&denied, 3);
    assert!(!String::from_utf8_lossy(&denied.stderr).contains("bad-token"));
}

#[tokio::test]
async fn real_binary_returns_three_for_usage_http_and_network_errors() {
    let server = Server::start(false, true).await;
    server.seed(None).await;
    for args in [
        vec![],
        vec!["--pacticipant", "C", "--to-environment", "test"],
        vec!["--pacticipant", "C", "--version", "1"],
        vec![
            "--pacticipant",
            "C",
            "--version",
            "1",
            "--main-branch",
            "--to-environment",
            "test",
        ],
        vec![
            "--pacticipant",
            "C",
            "--version",
            "1",
            "--to-environment",
            "test",
            "--to",
            "dev",
        ],
        vec![
            "--pacticipant",
            "C",
            "--version",
            "1",
            "--to-environment",
            "test",
            "--output",
            "xml",
        ],
        vec![
            "--pacticipant",
            "C",
            "--version",
            "1",
            "--to-environment",
            "test",
            "--broker-username",
            "reader",
        ],
        vec![
            "--pacticipant",
            "C",
            "--version",
            "1",
            "--to-environment",
            "test",
            "--broker-password",
            "secret-test",
        ],
        vec![
            "--pacticipant",
            "C",
            "--version",
            "1",
            "--to-environment",
            "test",
            "--ignore",
            "=2",
        ],
        vec![
            "--pacticipant",
            "absent",
            "--version",
            "1",
            "--to-environment",
            "test",
        ],
        vec![
            "--pacticipant",
            "C",
            "--version",
            "1",
            "--to-environment",
            "absent",
        ],
    ] {
        assert_code(&explain(&server.base, &args, &[]).await, 3);
    }
    assert_code(&explain("", PIN, &[]).await, 3);
    assert_code(
        &explain(&format!("http://127.0.0.1:{}", free_port()), PIN, &[]).await,
        3,
    );
}

#[tokio::test]
async fn real_binary_preserves_name_and_version_specific_ignore_semantics() {
    for success in [Some(false), None] {
        let server = Server::start(false, true).await;
        server.seed(success).await;
        for (ignore, code, ignored) in [
            ("P", 0, 1),
            (
                "P=2",
                if success.is_none() { 2 } else { 0 },
                if success.is_none() { 0 } else { 1 },
            ),
        ] {
            let mut args = PIN.to_vec();
            args.extend(["--ignore", ignore, "--output", "json"]);
            let output = explain(&server.base, &args, &[]).await;
            assert_code(&output, code);
            let doc: Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(doc["compatibility"]["ignored"], ignored);
        }
        let mut args = PIN.to_vec();
        args.extend(["--ignore", "P=99", "--ignore", "P", "--output", "json"]);
        let output = explain(&server.base, &args, &[]).await;
        assert_code(&output, 0);
        let doc: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(doc["resolution"]["ignores"][0]["version"], "99");
        assert_eq!(doc["resolution"]["ignores"][1]["version"], Value::Null);
    }
}

#[tokio::test]
async fn real_binary_rejects_empty_conflicting_and_repeated_options() {
    let server = Server::start(false, true).await;
    server.seed(Some(true)).await;
    for extra in [
        vec!["--ignore", ""],
        vec!["--ignore", "P="],
        vec!["--pacticipant", "Other"],
        vec![
            "--broker-token",
            "token-test",
            "--broker-username",
            "u",
            "--broker-password",
            "p",
        ],
        vec!["--unknown-option"],
    ] {
        let mut args = PIN.to_vec();
        args.extend(extra);
        assert_code(&explain(&server.base, &args, &[]).await, 3);
    }
    for (key, value) in [
        ("--version", ""),
        ("--pacticipant", ""),
        ("--to-environment", ""),
    ] {
        let mut args = PIN.to_vec();
        let index = args.iter().position(|a| *a == key).unwrap();
        args[index + 1] = value;
        assert_code(&explain(&server.base, &args, &[]).await, 3);
    }
}

/// Controlled responses exercise protocol rejection; verdict behavior above uses the real broker.
#[tokio::test]
async fn client_never_approves_malformed_or_inconsistent_protocol_responses() {
    let valid: Value =
        serde_json::from_str(include_str!("fixtures/pactling/explanations/allowed.json")).unwrap();
    let mut mismatched = valid.clone();
    mismatched["compatibility"]["deployable"] = Value::Null;
    let mut invalid_decision = valid.clone();
    invalid_decision["decision"] = json!("maybe");
    let mut unsupported = valid.clone();
    unsupported["schemaVersion"] = json!("2.0");
    let mut missing = valid.clone();
    missing.as_object_mut().unwrap().remove("decision");
    for (status,body) in [
        (200,"not JSON".to_owned()),
        (200,"{}".to_owned()),
        (200,mismatched.to_string()),
        (200,invalid_decision.to_string()),
        (200,unsupported.to_string()),
        (200,missing.to_string()),
        (401,valid.to_string()),
        (400,json!({"schemaVersion":"1.0","error":{"code":"INVALID_QUERY","details":{"version":["is missing"]}}}).to_string()),
        (500,valid.to_string()),
    ] {
        let router=axum::Router::new().fallback(move || {let body=body.clone();async move {(axum::http::StatusCode::from_u16(status).unwrap(),[("content-type","application/json")],body)}});
        let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base=format!("http://{}",listener.local_addr().unwrap());
        let task=tokio::spawn(async move {axum::serve(listener,router).await.unwrap()});
        let output=explain(&base,PIN,&[]).await;task.abort();
        assert_code(&output,3);
    }
}

#[tokio::test]
async fn explain_help_hides_secret_environment_values_and_returns_zero() {
    let output = explain(
        "",
        &["--help"],
        &[
            ("PACT_BROKER_BASE_URL", "https://broker.example"),
            ("PACT_BROKER_USERNAME", "reader"),
            ("PACT_BROKER_PASSWORD", "disposable-help-password"),
            ("PACT_BROKER_TOKEN", "disposable-help-token"),
        ],
    )
    .await;
    assert_code(&output, 0);
    let help = String::from_utf8(output.stdout).unwrap();
    assert!(help.contains("--pacticipant") && help.contains("--main-branch"));
    assert!(
        !help.contains("disposable-help-password") && !help.contains("disposable-help-token"),
        "{help}"
    );
}

#[tokio::test]
async fn explicit_connection_and_auth_flags_override_their_environment_values() {
    let server = Server::start(true, false).await;
    server.seed(Some(true)).await;
    let mut basic = PIN.to_vec();
    basic.extend([
        "--broker-username",
        "reader",
        "--broker-password",
        "disposable-read",
    ]);
    assert_code(
        &explain(
            &server.base,
            &basic,
            &[
                ("PACT_BROKER_BASE_URL", "http://127.0.0.1:1"),
                ("PACT_BROKER_USERNAME", "wrong-user"),
                ("PACT_BROKER_PASSWORD", "wrong-password"),
            ],
        )
        .await,
        0,
    );
    let mut token = PIN.to_vec();
    token.extend(["--broker-token", "disposable-token"]);
    assert_code(
        &explain(
            &server.base,
            &token,
            &[("PACT_BROKER_TOKEN", "wrong-token")],
        )
        .await,
        0,
    );
}

#[tokio::test]
async fn client_does_not_follow_redirects_to_a_success_document() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let hits = Arc::new(AtomicUsize::new(0));
    let observed = hits.clone();
    let router = axum::Router::new()
        .route(
            "/destination",
            axum::routing::get(move || {
                let observed = observed.clone();
                async move {
                    observed.fetch_add(1, Ordering::SeqCst);
                    (
                        [("content-type", "application/json")],
                        include_str!("fixtures/pactling/explanations/allowed.json"),
                    )
                }
            }),
        )
        .fallback(|| async {
            (
                axum::http::StatusCode::FOUND,
                [("location", "/destination")],
                "redirect",
            )
        });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let output = explain(&base, PIN, &[]).await;
    task.abort();
    assert_code(&output, 3);
    assert_eq!(
        hits.load(Ordering::SeqCst),
        0,
        "redirect must not be followed"
    );
}
