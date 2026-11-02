#![cfg(feature = "mcp")]
use std::process::Stdio;
use tokio::{
    process::Command,
    time::{Duration, timeout},
};

#[tokio::test]
async fn mcp_help_is_stderr_only_and_invalid_options_are_redacted() {
    for (args, success) in [
        (vec!["mcp", "--help"], true),
        (vec!["mcp", "--unknown-secret=do-not-echo"], false),
    ] {
        let output = timeout(
            Duration::from_secs(5),
            Command::new(env!("CARGO_BIN_EXE_pactling"))
                .args(args)
                .env_remove("PACT_BROKER_BASE_URL")
                .stdin(Stdio::null())
                .kill_on_drop(true)
                .output(),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(output.status.success(), success);
        assert!(
            output.stdout.is_empty(),
            "MCP command must keep stdout protocol-only"
        );
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(!stderr.is_empty());
        assert!(!stderr.contains("do-not-echo"));
        if success {
            assert!(stderr.contains("--broker-base-url"));
        }
    }
}
