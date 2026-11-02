#![cfg(feature = "mcp")]
use pactling::broker_client::{Credentials, Grouping, ReadClient, ReadRoute};
use std::process::Stdio;
use tokio::{
    process::Command,
    time::{Duration, timeout},
};

#[test]
fn mcp_matrix_query_grouping_and_query_only_names_are_exact() {
    let client = ReadClient::new("https://broker.example/prefix", Credentials::default()).unwrap();
    for (grouping, value) in [
        (None, None),
        (Some(Grouping::Cvpv), Some("cvpv")),
        (Some(Grouping::Cvp), Some("cvp")),
        (Some(Grouping::Cp), Some("cp")),
    ] {
        let url = client
            .source_url(&ReadRoute::Matrix {
                participant_a: "..".into(),
                participant_b: "A /%? ü".into(),
                latestby: grouping,
            })
            .unwrap();
        assert_eq!(url.path(), "/prefix/matrix");
        let mut expected = vec![
            ("q[][pacticipant]".to_owned(), "..".to_owned()),
            ("q[][pacticipant]".into(), "A /%? ü".into()),
        ];
        if let Some(value) = value {
            expected.push(("latestby".into(), value.into()));
        }
        assert_eq!(url.query_pairs().into_owned().collect::<Vec<_>>(), expected);
    }
}

#[tokio::test]
async fn mcp_pact_and_explanation_accept_plain_json_catalogs_accept_hal() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let app = axum::Router::new().fallback(|request: axum::extract::Request| async move {
        request
            .headers()
            .get("accept")
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned()
    });
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let client = ReadClient::new(&base, Credentials::default()).unwrap();
    use pactling::broker_client::{ExplanationQuery, Selector, Target};
    for (route, expected) in [
        (ReadRoute::Integrations, "application/hal+json"),
        (ReadRoute::Pacticipants, "application/hal+json"),
        (
            ReadRoute::Matrix {
                participant_a: "A".into(),
                participant_b: "B".into(),
                latestby: None,
            },
            "application/hal+json",
        ),
        (
            ReadRoute::Pact {
                consumer: "A".into(),
                provider: "B".into(),
                version: "1".into(),
            },
            "application/json",
        ),
        (
            ReadRoute::Explanation(ExplanationQuery {
                pacticipant: "A".into(),
                selector: Selector::Version("1".into()),
                target: Target::Tag("prod".into()),
                ignores: vec![],
            }),
            "application/json",
        ),
    ] {
        assert_eq!(client.get(&route).await.unwrap().bytes, expected.as_bytes());
    }
    server.abort();
}

#[tokio::test]
async fn explain_preserves_disabled_response_decoding_with_mcp_enabled() {
    for (encoding, compress) in [("gzip", "gzip.compress"), ("deflate", "zlib.compress")] {
        let compressed=Command::new("python3").args(["-c",&format!("import sys,gzip,zlib;sys.stdout.buffer.write({compress}(sys.stdin.buffer.read()))")]).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
        let mut compressed = compressed;
        use tokio::io::AsyncWriteExt;
        let mut input = compressed.stdin.take().unwrap();
        input
            .write_all(include_bytes!(
                "fixtures/pactling/explanations/allowed.json"
            ))
            .await
            .unwrap();
        drop(input);
        let bytes = compressed.wait_with_output().await.unwrap().stdout;
        let app = axum::Router::new().fallback(move || {
            let bytes = bytes.clone();
            async move { ([(axum::http::header::CONTENT_ENCODING, encoding)], bytes) }
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let result = timeout(
            Duration::from_secs(5),
            Command::new(env!("CARGO_BIN_EXE_pactling"))
                .args([
                    "explain",
                    "--broker-base-url",
                    &base,
                    "--pacticipant",
                    "A",
                    "--version",
                    "1",
                    "--to-environment",
                    "test",
                    "--output",
                    "json",
                ])
                .env_remove("PACT_BROKER_USERNAME")
                .env_remove("PACT_BROKER_PASSWORD")
                .env_remove("PACT_BROKER_TOKEN")
                .kill_on_drop(true)
                .output(),
        )
        .await
        .unwrap()
        .unwrap();
        server.abort();
        assert_eq!(
            result.status.code(),
            Some(3),
            "existing CLI must not start decoding {encoding}"
        );
        assert!(result.stdout.is_empty());
        assert!(
            String::from_utf8(result.stderr)
                .unwrap()
                .contains("broker returned invalid JSON")
        );
    }
}

#[tokio::test]
async fn mcp_valid_auth_flags_initialize_without_fetching_broker() {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    for args in [
        vec![
            "--broker-username",
            "reader",
            "--broker-password",
            "synthetic-secret",
        ],
        vec!["--broker-token", "synthetic-token"],
    ] {
        let mut child = Command::new(env!("CARGO_BIN_EXE_pactling"))
            .args(["mcp", "--broker-base-url", "http://127.0.0.1:1"])
            .args(args)
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
        let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
        input.write_all(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"protocolVersion\":\"2025-06-18\",\"capabilities\":{},\"clientInfo\":{\"name\":\"auth-test\",\"version\":\"1\"}}}\n").await.unwrap();
        let line = timeout(Duration::from_secs(3), lines.next_line())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let response: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(response["result"]["protocolVersion"], "2025-06-18");
        drop(input);
        child.kill().await.unwrap();
        child.wait().await.unwrap();
    }
}

#[tokio::test]
async fn webhook_preserves_disabled_response_decoding_with_mcp_enabled() {
    use clap::Parser;
    use pactling::{
        config::Config,
        service::webhook_delivery::{self, BuiltRequest, Messages},
    };
    use std::sync::{Arc, Mutex};
    for (encoding, compress) in [("gzip", "gzip.compress"), ("deflate", "zlib.compress")] {
        let output = Command::new("python3")
            .args([
                "-c",
                &format!(
                    "import sys,gzip,zlib;sys.stdout.buffer.write({compress}(b'recorded-response'))"
                ),
            ])
            .output()
            .await
            .unwrap();
        assert!(output.status.success());
        let bytes = output.stdout;
        let expected = String::from_utf8_lossy(&bytes).into_owned();
        let seen = Arc::new(Mutex::new(None));
        let capture = seen.clone();
        let app = axum::Router::new().fallback(move |request: axum::extract::Request| {
            let bytes = bytes.clone();
            let capture = capture.clone();
            async move {
                *capture.lock().unwrap() = request.headers().get("accept-encoding").cloned();
                ([(axum::http::header::CONTENT_ENCODING, encoding)], bytes)
            }
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let config = Config::parse_from(["pactling"]);
        let result = webhook_delivery::execute(
            &config,
            None,
            &BuiltRequest {
                method: "POST".into(),
                url,
                headers: vec![],
                body: "null".into(),
            },
            &serde_json::Map::new(),
            &Messages::default(),
        )
        .await;
        server.abort();
        assert!(
            seen.lock().unwrap().is_none(),
            "MCP feature must not change webhook Accept-Encoding"
        );
        let response = result.response.unwrap();
        assert!(
            response
                .headers
                .iter()
                .any(|(key, value)| key == "content-encoding" && value == encoding)
        );
        assert_eq!(
            response.body, expected,
            "preserve original raw response handling for {encoding}"
        );
    }
}
