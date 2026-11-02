#![cfg(feature = "mcp")]
use pactling::broker_client::{ClientError, Credentials, Grouping, ReadClient, ReadRoute};
use std::{
    process::Stdio,
    sync::{Arc, Mutex},
};
use tokio::{
    process::Command,
    time::{Duration, timeout},
};

type CapturedRequests = Arc<Mutex<Vec<(String, String, String)>>>;

async fn upstream(
    status: u16,
    headers: Vec<(&'static str, String)>,
    body: Vec<u8>,
) -> (String, CapturedRequests, tokio::task::JoinHandle<()>) {
    let seen = Arc::new(Mutex::new(vec![]));
    let capture = seen.clone();
    let app = axum::Router::new().fallback(move |request: axum::extract::Request| {
        let capture = capture.clone();
        let headers = headers.clone();
        let body = body.clone();
        async move {
            capture.lock().unwrap().push((
                request.method().to_string(),
                request.uri().to_string(),
                request
                    .headers()
                    .get("authorization")
                    .map(|v| v.to_str().unwrap().to_owned())
                    .unwrap_or_default(),
            ));
            let mut response = axum::http::Response::builder().status(status);
            for (key, value) in headers {
                response = response.header(key, value);
            }
            response.body(axum::body::Body::from(body)).unwrap()
        }
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (base, seen, handle)
}
fn client(base: &str) -> ReadClient {
    ReadClient::new(base, Credentials::default()).unwrap()
}
fn pact(name: &str) -> ReadRoute {
    ReadRoute::Pact {
        consumer: name.into(),
        provider: name.into(),
        version: name.into(),
    }
}

#[tokio::test]
async fn mcp_reuses_basic_token_and_read_only_auth() {
    let (base, seen, server) = upstream(200, vec![], b"{}".to_vec()).await;
    for (credentials, expected) in [
        (Credentials::default(), ""),
        (
            Credentials {
                username: Some("reader".into()),
                password: Some("secret".into()),
                token: None,
            },
            "Basic cmVhZGVyOnNlY3JldA==",
        ),
        (
            Credentials {
                token: Some("synthetic-token".into()),
                ..Default::default()
            },
            "Bearer synthetic-token",
        ),
    ] {
        let response = ReadClient::new(&base, credentials)
            .unwrap()
            .get(&ReadRoute::Integrations)
            .await;
        assert!(response.is_ok(), "GET must be supported: {response:?}");
        assert_eq!(seen.lock().unwrap().last().unwrap().2, expected);
    }
    server.abort();
}
#[tokio::test]
async fn mcp_rejects_url_credentials_and_redirects() {
    for base in [
        "bad",
        "ftp://host",
        "http://user:synthetic-secret@host",
        "http://user@host",
        "http://:secret@host",
        "http://host?x=1",
        "http://host#x",
    ] {
        assert!(
            ReadClient::new(base, Credentials::default()).is_err(),
            "accepted {base}"
        );
    }
    let (base, seen, server) =
        upstream(302, vec![("location", "/destination".into())], vec![]).await;
    let response = client(&base).get(&ReadRoute::Integrations).await.unwrap();
    assert_eq!(response.status, 302);
    assert_eq!(seen.lock().unwrap().len(), 1);
    server.abort();
}
#[tokio::test]
async fn mcp_never_accepts_tool_urls_or_write_methods() {
    let (base, seen, server) = upstream(200, vec![], b"{}".to_vec()).await;
    let c = client(&base);
    let routes = [
        ReadRoute::Integrations,
        ReadRoute::Pacticipants,
        pact("https://evil.invalid/write?method=POST"),
        ReadRoute::Matrix {
            participant_a: "A".into(),
            participant_b: "B".into(),
            latestby: Some(Grouping::Cp),
        },
        ReadRoute::Explanation(pactling::broker_client::ExplanationQuery {
            pacticipant: "A".into(),
            selector: pactling::broker_client::Selector::MainBranch,
            target: pactling::broker_client::Target::Environment("test".into()),
            ignores: vec![],
        }),
    ];
    for route in routes {
        assert!(c.get(&route).await.is_ok());
    }
    assert_eq!(seen.lock().unwrap().len(), 5);
    assert!(
        seen.lock()
            .unwrap()
            .iter()
            .all(|(method, _, _)| method == "GET")
    );
    server.abort();
}
#[test]
fn mcp_client_exposes_only_get() {
    let source = include_str!("../src/broker_client.rs");
    assert!(!source.contains("pub client: reqwest"));
    assert!(!source.contains("pub fn request("));
    assert!(!source.contains("pub async fn post("));
    assert!(
        source.contains("client: reqwest::Client"),
        "underlying client must stay private"
    );
}
#[test]
fn mcp_encoded_names_preserve_exact_request_target() {
    let c = client("https://example.com/broker%20path///");
    for (name, encoded) in [
        ("a/b", "a%2Fb"),
        ("%2F", "%252F"),
        ("a?b#c", "a%3Fb%23c"),
        ("ü x", "%C3%BC%20x"),
        ("https://evil.invalid", "https:%2F%2Fevil.invalid"),
    ] {
        let url = c.source_url(&pact(name));
        assert!(url.is_ok(), "encoded URL must be available: {url:?}");
        assert_eq!(
            url.unwrap().as_str(),
            format!(
                "https://example.com/broker%20path/pacts/provider/{encoded}/consumer/{encoded}/version/{encoded}"
            )
        );
    }
}
#[test]
fn mcp_rejects_dot_and_control_path_segments() {
    let c = client("http://localhost");
    for name in [
        ".", "..", "a\nb", "a\rb", "a\tb", "a\0b", "a\u{7f}b", "a\u{85}b",
    ] {
        assert_eq!(
            c.source_url(&pact(name)).unwrap_err(),
            ClientError::InvalidPath
        );
    }
    assert!(c.source_url(&pact("...%2E")).is_ok());
}
#[tokio::test]
async fn mcp_bounds_response_reads_and_redacts_errors() {
    for size in [4 * 1024 * 1024, 4 * 1024 * 1024 + 1] {
        let (base, _, server) = upstream(200, vec![], vec![b'x'; size]).await;
        let response = client(&base).get(&ReadRoute::Integrations).await;
        if size == 4 * 1024 * 1024 {
            assert_eq!(response.unwrap().bytes.len(), size);
        } else {
            assert_eq!(response.unwrap_err(), ClientError::ResponseTooLarge);
        }
        server.abort();
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!(
        "http://{}/synthetic-private-path",
        listener.local_addr().unwrap()
    );
    drop(listener);
    let error = client(&base)
        .get(&ReadRoute::Integrations)
        .await
        .unwrap_err();
    assert_eq!(error, ClientError::Unavailable);
    assert!(!error.to_string().contains("synthetic"));
    let (base, _, server) = upstream(401, vec![], b"synthetic-secret".to_vec()).await;
    let response = client(&base).get(&ReadRoute::Integrations).await.unwrap();
    assert_eq!(response.status, 401); // Safe seam retains bytes for task3's fixed-message mapping.
    server.abort();
}
#[tokio::test]
async fn mcp_decoded_body_limit_rejects_compression_bombs() {
    // Python's stdlib creates real gzip/zlib streams; reqwest must decode before counting.
    for (encoding, code) in [
        ("gzip", "gzip.compress(b'x'*(4*1024*1024+1))"),
        ("deflate", "zlib.compress(b'x'*(4*1024*1024+1))"),
    ] {
        let output = Command::new("python3")
            .args([
                "-c",
                &format!("import sys,gzip,zlib;sys.stdout.buffer.write({code})"),
            ])
            .output()
            .await
            .unwrap();
        assert!(output.status.success());
        assert!(output.stdout.len() < 8192);
        let (base, _, server) = upstream(
            200,
            vec![("content-encoding", encoding.into())],
            output.stdout,
        )
        .await;
        assert_eq!(
            client(&base)
                .get(&ReadRoute::Integrations)
                .await
                .unwrap_err(),
            ClientError::ResponseTooLarge
        );
        server.abort();
    }
}
#[tokio::test]
async fn mcp_invalid_startup_config_exits_nonzero_stderr_only() {
    for env in [
        vec![],
        vec![(
            "PACT_BROKER_BASE_URL",
            "http://synthetic-user:synthetic-secret@localhost",
        )],
        vec![
            ("PACT_BROKER_BASE_URL", "http://localhost"),
            ("PACT_BROKER_USERNAME", "synthetic-user"),
        ],
        vec![
            ("PACT_BROKER_BASE_URL", "http://localhost"),
            ("PACT_BROKER_PASSWORD", "synthetic-secret"),
        ],
        vec![
            ("PACT_BROKER_BASE_URL", "http://localhost"),
            ("PACT_BROKER_USERNAME", "synthetic-user"),
            ("PACT_BROKER_PASSWORD", "synthetic-secret"),
            ("PACT_BROKER_TOKEN", "synthetic-token"),
        ],
    ] {
        let mut c = Command::new(env!("CARGO_BIN_EXE_pactling"));
        c.arg("mcp")
            .env_remove("PACT_BROKER_BASE_URL")
            .env_remove("PACT_BROKER_USERNAME")
            .env_remove("PACT_BROKER_PASSWORD")
            .env_remove("PACT_BROKER_TOKEN")
            .envs(env)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let mut child = c.spawn().unwrap();
        let input = child.stdin.take().unwrap();
        let output = timeout(Duration::from_secs(2), child.wait_with_output()).await;
        drop(input);
        assert!(
            output.is_ok(),
            "invalid startup must exit before initialize"
        );
        let output = output.unwrap().unwrap();
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(!stderr.is_empty());
        assert!(!stderr.contains("synthetic"), "{stderr}");
    }
}

#[test]
fn mcp_auth_configuration_requires_one_complete_identity() {
    for credentials in [
        Credentials {
            username: Some("u".into()),
            ..Default::default()
        },
        Credentials {
            password: Some("p".into()),
            ..Default::default()
        },
        Credentials {
            username: Some("u".into()),
            password: Some("p".into()),
            token: Some("t".into()),
        },
        Credentials {
            username: Some("".into()),
            password: Some("p".into()),
            token: None,
        },
        Credentials {
            token: Some("".into()),
            ..Default::default()
        },
    ] {
        assert!(ReadClient::new("http://localhost", credentials).is_err());
    }
    // The existing CLI permits an empty Basic password; retain that contract.
    assert!(
        ReadClient::new(
            "http://localhost",
            Credentials {
                username: Some("u".into()),
                password: Some("".into()),
                token: None
            }
        )
        .is_ok()
    );
}
#[tokio::test]
async fn mcp_encoded_names_are_sent_without_reinterpretation() {
    let (base, seen, server) = upstream(200, vec![], vec![]).await;
    client(&format!("{base}/broker%20path/"))
        .get(&pact("a/%2F?#ü"))
        .await
        .unwrap();
    assert_eq!(
        seen.lock().unwrap()[0].1,
        "/broker%20path/pacts/provider/a%2F%252F%3F%23%C3%BC/consumer/a%2F%252F%3F%23%C3%BC/version/a%2F%252F%3F%23%C3%BC"
    );
    server.abort();
}
#[test]
fn mcp_typed_explanation_queries_match_recorded_cli_pairs() {
    use clap::Parser;
    use pactling::broker_client::{ExplanationQuery, Selector, Target};
    let cases: serde_json::Value = serde_json::from_str(include_str!(
        "fixtures/cli/explain-can-i-deploy-queries.json"
    ))
    .unwrap();
    for case in cases.as_array().unwrap() {
        let mut argv = vec!["explain".to_owned()];
        argv.extend(
            case["explainOptions"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_str().unwrap().to_owned()),
        );
        argv.extend(["--broker-base-url".into(), "http://localhost".into()]);
        let options = pactling::cli::Explain::try_parse_from(argv).unwrap();
        let selector = if let Some(v) = options.version {
            Selector::Version(v)
        } else if let Some(tag) = options.latest {
            Selector::Latest(if tag.is_empty() { None } else { Some(tag) })
        } else if let Some(branch) = options.branch {
            Selector::Branch(branch)
        } else {
            Selector::MainBranch
        };
        let target = if let Some(env) = options.to_environment {
            Target::Environment(env)
        } else {
            Target::Tag(options.to.unwrap())
        };
        let query = ExplanationQuery {
            pacticipant: options.pacticipant,
            selector,
            target,
            ignores: options.ignore,
        };
        let recorded = reqwest::Url::parse(&format!(
            "http://broker{}",
            case["requestPath"].as_str().unwrap()
        ))
        .unwrap();
        let expected: Vec<(String, String)> = recorded.query_pairs().into_owned().collect();
        assert_eq!(query.pairs(), expected, "{case}");
    }
}
#[tokio::test]
async fn mcp_chunked_body_is_bounded_without_content_length() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = [0; 4096];
        assert!(socket.read(&mut request).await.unwrap() > 0);
        socket
            .write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n")
            .await
            .unwrap();
        let chunk = vec![b'x'; 65536];
        for _ in 0..65 {
            if socket.write_all(b"10000\r\n").await.is_err() {
                return;
            }
            if socket.write_all(&chunk).await.is_err() {
                return;
            }
            if socket.write_all(b"\r\n").await.is_err() {
                return;
            }
        }
        // Leave the response unfinished: limit must stop the read before EOF.
        tokio::time::sleep(Duration::from_secs(10)).await;
    });
    let result = timeout(
        Duration::from_secs(2),
        client(&base).get(&ReadRoute::Integrations),
    )
    .await
    .unwrap();
    assert_eq!(result.unwrap_err(), ClientError::ResponseTooLarge);
    server.abort();
}

#[tokio::test]
async fn mcp_transport_retains_thirty_second_timeout() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (_socket, _) = listener.accept().await.unwrap();
        tokio::time::sleep(Duration::from_secs(40)).await;
    });
    let started = std::time::Instant::now();
    let response = timeout(
        Duration::from_secs(33),
        client(&base).get(&ReadRoute::Integrations),
    )
    .await;
    server.abort();
    assert!(response.is_ok(), "request must time out within 33 seconds");
    assert_eq!(response.unwrap().unwrap_err(), ClientError::Timeout);
    assert!(
        started.elapsed() >= Duration::from_secs(29),
        "retain the existing 30-second timeout"
    );
}
