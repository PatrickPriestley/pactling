use std::process::Stdio;
use tokio::process::Command;
use tokio::time::{Duration, timeout};

fn command() -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_pactling"));
    c.arg("mcp")
        .env_remove("PACT_BROKER_BASE_URL")
        .env_remove("PACT_BROKER_USERNAME")
        .env_remove("PACT_BROKER_PASSWORD")
        .env_remove("PACT_BROKER_TOKEN")
        .env("DATABASE_URL", "invalid://mcp-must-not-open-a-database")
        .env("RUST_LOG", "trace")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    c
}

#[cfg(not(feature = "mcp"))]
#[tokio::test]
async fn mcp_feature_disabled_reports_unavailable() {
    let output = timeout(Duration::from_secs(5), command().output())
        .await
        .unwrap()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("MCP support is not enabled"), "{stderr}");
    assert!(stderr.contains("--features mcp"), "{stderr}");
}

#[cfg(feature = "mcp")]
mod enabled {
    use super::*;
    use serde_json::{Value, json};
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    async fn session(
        protocol: &str,
    ) -> (
        tokio::process::Child,
        tokio::process::ChildStdin,
        tokio::io::Lines<BufReader<tokio::process::ChildStdout>>,
    ) {
        let mut child = command()
            .args(["--broker-base-url", "http://127.0.0.1:1"])
            .spawn()
            .unwrap();
        let mut input = child.stdin.take().unwrap();
        let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
        send(&mut input, json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":protocol,"capabilities":{},"clientInfo":{"name":"dispatch-test","version":"1"}}})).await;
        let reply = read(&mut lines).await;
        assert_eq!(reply["result"]["protocolVersion"], protocol);
        assert!(reply["result"]["capabilities"]["tools"].is_object());
        assert!(reply["result"]["capabilities"].get("resources").is_none());
        assert!(reply["result"]["capabilities"].get("prompts").is_none());
        send(
            &mut input,
            json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        )
        .await;
        (child, input, lines)
    }
    async fn send(input: &mut tokio::process::ChildStdin, message: Value) {
        input
            .write_all(format!("{message}\n").as_bytes())
            .await
            .unwrap();
        input.flush().await.unwrap();
    }
    async fn read(lines: &mut tokio::io::Lines<BufReader<tokio::process::ChildStdout>>) -> Value {
        let line = timeout(Duration::from_secs(5), lines.next_line())
            .await
            .expect("MCP reply timeout")
            .unwrap()
            .expect("MCP reply must exist");
        serde_json::from_str(&line).expect("stdout contains JSON-RPC frames only")
    }
    #[tokio::test]
    async fn mcp_stdio_initializes_and_lists_only_five_read_tools() {
        let (mut child, mut input, mut lines) = session("2025-06-18").await;
        send(
            &mut input,
            json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
        )
        .await;
        let reply = read(&mut lines).await;
        let tools = reply["result"]["tools"].as_array().unwrap();
        assert_eq!(
            tools
                .iter()
                .map(|t| t["name"].as_str().unwrap())
                .collect::<Vec<_>>(),
            vec![
                "pactling_explain_deployment",
                "pactling_get_pact",
                "pactling_list_integrations",
                "pactling_list_pacticipants",
                "pactling_matrix"
            ]
        );
        for tool in tools {
            assert_eq!(
                tool["annotations"],
                json!({"readOnlyHint":true,"destructiveHint":false,"idempotentHint":true,"openWorldHint":false})
            );
            assert_eq!(tool["inputSchema"]["type"], "object");
        }
        assert!(reply["result"].get("nextCursor").is_none());
        drop(input);
        assert!(
            timeout(Duration::from_secs(5), child.wait())
                .await
                .unwrap()
                .unwrap()
                .success()
        );
        assert!(lines.next_line().await.unwrap().is_none());
    }
    #[tokio::test]
    async fn mcp_stdio_negotiates_2025_11_25() {
        let (mut child, input, _) = session("2025-11-25").await;
        drop(input);
        assert!(
            timeout(Duration::from_secs(5), child.wait())
                .await
                .unwrap()
                .unwrap()
                .success()
        );
    }
    #[tokio::test]
    async fn mcp_stdout_contains_only_frames() {
        let (mut child, mut input, mut lines) = session("2025-06-18").await;
        send(&mut input, json!({"jsonrpc":"2.0","id":9,"method":"ping"})).await;
        let reply = read(&mut lines).await;
        assert_eq!(reply["id"], 9);
        assert_eq!(reply["result"], json!({}));
        drop(input);
        assert!(
            timeout(Duration::from_secs(5), child.wait())
                .await
                .unwrap()
                .unwrap()
                .success()
        );
        assert!(lines.next_line().await.unwrap().is_none());
    }
}
