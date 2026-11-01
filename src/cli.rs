//! Client-side deployment explanations. Server configuration remains independent.
use clap::{ArgGroup, Parser};
mod error;
mod text;

#[derive(Parser)]
#[command(name="pactling explain", disable_version_flag=true,
    group(ArgGroup::new("selector").required(true).args(["version", "latest", "branch", "main_branch"])),
    group(ArgGroup::new("target").required(true).args(["to_environment", "to"])))]
pub struct Explain {
    #[arg(long, short='a', value_parser=nonempty)]
    pub pacticipant: String,
    #[arg(long, short='e', value_parser=nonempty)]
    pub version: Option<String>,
    #[arg(long, short='l', num_args=0..=1, default_missing_value="")]
    pub latest: Option<String>,
    #[arg(long, value_parser=nonempty)]
    pub branch: Option<String>,
    #[arg(long)]
    pub main_branch: bool,
    #[arg(long, value_parser=nonempty)]
    pub to_environment: Option<String>,
    #[arg(long, value_parser=nonempty)]
    pub to: Option<String>,
    #[arg(long, value_parser=parse_ignore)]
    pub ignore: Vec<Ignore>,
    #[arg(long, short='b', env="PACT_BROKER_BASE_URL", value_parser=nonempty)]
    pub broker_base_url: String,
    #[arg(long, short='u', env="PACT_BROKER_USERNAME", requires="broker_password", value_parser=nonempty)]
    pub broker_username: Option<String>,
    #[arg(
        long,
        short = 'p',
        env = "PACT_BROKER_PASSWORD",
        requires = "broker_username",
        hide_env_values = true
    )]
    pub broker_password: Option<String>,
    #[arg(long, short='k', env="PACT_BROKER_TOKEN", conflicts_with_all=["broker_username","broker_password"], hide_env_values=true, value_parser=nonempty)]
    pub broker_token: Option<String>,
    #[arg(long, short='o', default_value="text", value_parser=["text","json"])]
    pub output: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ignore {
    pub name: String,
    pub version: Option<String>,
}
fn nonempty(value: &str) -> Result<String, String> {
    if value.is_empty() {
        Err("must not be empty".into())
    } else {
        Ok(value.into())
    }
}
fn parse_ignore(value: &str) -> Result<Ignore, String> {
    let (name, version) = match value.split_once('=') {
        Some((n, v)) => (n, Some(nonempty(v)?)),
        None => (value, None),
    };
    Ok(Ignore {
        name: nonempty(name)?,
        version,
    })
}

pub fn query_pairs(args: &Explain) -> Vec<(String, String)> {
    let mut pairs = vec![];
    let mut add = |key: &str, value: &str| pairs.push((key.to_owned(), value.to_owned()));
    add("q[][pacticipant]", &args.pacticipant);
    if let Some(version) = &args.version {
        add("q[][version]", version);
    } else {
        add("q[][latest]", "true");
        if let Some(tag) = &args.latest {
            if !tag.is_empty() {
                add("q[][tag]", tag);
            }
        } else if let Some(branch) = &args.branch {
            add("q[][branch]", branch);
        } else {
            add("q[][mainBranch]", "true");
        }
    }
    add("latestby", "cvp");
    if let Some(environment) = &args.to_environment {
        add("environment", environment);
    } else {
        add("latest", "true");
        add("tag", args.to.as_deref().expect("required target group"));
    }
    for ignore in &args.ignore {
        add("ignore[][pacticipant]", &ignore.name);
        if let Some(version) = &ignore.version {
            add("ignore[][version]", version);
        }
    }
    pairs
}

fn decision_code(document: &serde_json::Value) -> Result<i32, String> {
    let invalid = || "broker returned an invalid or unsupported explanation document".to_string();
    if !document["schemaVersion"]
        .as_str()
        .is_some_and(|v| v.starts_with("1."))
        || !document["reasons"].is_array()
        || !document["nextActions"].is_array()
    {
        return Err(invalid());
    }
    let (code, deployable) = match document["decision"].as_str() {
        Some("allowed") => (0, serde_json::json!(true)),
        Some("blocked") => (1, serde_json::json!(false)),
        Some("unknown") => (2, serde_json::Value::Null),
        _ => return Err(invalid()),
    };
    if document["compatibility"].get("deployable") != Some(&deployable) {
        return Err(invalid());
    }
    Ok(code)
}

fn render_text(document: &serde_json::Value) -> String {
    text::render(document)
}

fn endpoint(args: &Explain) -> Result<reqwest::Url, String> {
    let mut url =
        reqwest::Url::parse(&args.broker_base_url).map_err(|_| "invalid broker base URL")?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(
            "broker base URL must be HTTP(S), without credentials, query or fragment".into(),
        );
    }
    let path = format!(
        "{}/pactling/v1/can-i-deploy/explanation",
        url.path().trim_end_matches('/')
    );
    url.set_path(&path);
    url.query_pairs_mut().extend_pairs(query_pairs(args));
    Ok(url)
}

async fn request(args: Explain) -> Result<i32, String> {
    use std::io::Write;
    let url = endpoint(&args)?;
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| e.without_url().to_string())?;
    let mut request = client
        .get(url)
        .header(reqwest::header::ACCEPT, "application/json");
    if let Some(username) = args.broker_username {
        request = request.basic_auth(username, args.broker_password);
    } else if let Some(token) = args.broker_token {
        request = request.bearer_auth(token);
    }
    let response = request
        .send()
        .await
        .map_err(|e| format!("request failed: {}", e.without_url()))?;
    let status = response.status();
    let bytes = response
        .bytes()
        .await
        .map_err(|e| format!("reading response failed: {}", e.without_url()))?;
    if !status.is_success() {
        if args.output == "json" {
            std::io::stdout()
                .write_all(&bytes)
                .map_err(|e| e.to_string())?;
        }
        return Err(if args.output == "json" {
            format!("broker returned HTTP {status}")
        } else {
            error::describe(status, &bytes)
        });
    }
    let document: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|_| "broker returned invalid JSON")?;
    let code = decision_code(&document)?;
    if args.output == "json" {
        std::io::stdout()
            .write_all(&bytes)
            .map_err(|e| e.to_string())?;
    } else {
        std::io::stdout()
            .write_all(render_text(&document).as_bytes())
            .map_err(|e| e.to_string())?;
    }
    Ok(code)
}

/// Help is successful; all explain usage/request failures have the documented code 3.
pub async fn run(args: impl IntoIterator<Item = std::ffi::OsString>) -> i32 {
    let options = match Explain::try_parse_from(args) {
        Ok(options) => options,
        Err(error) => {
            let code = if error.use_stderr() { 3 } else { 0 };
            let _ = error.print();
            return code;
        }
    };
    match request(options).await {
        Ok(code) => code,
        Err(message) => {
            eprintln!("pactling explain: {message}");
            3
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn option_queries_match_recorded_pact_broker_1_77_requests() {
        let cases: serde_json::Value = serde_json::from_str(include_str!(
            "../tests/fixtures/cli/explain-can-i-deploy-queries.json"
        ))
        .unwrap();
        for case in cases.as_array().unwrap() {
            let mut args = vec!["explain".to_string()];
            args.extend(
                case["explainOptions"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_str().unwrap().to_string()),
            );
            args.extend(["--broker-base-url".into(), "http://localhost:9292".into()]);
            let options = Explain::try_parse_from(args).unwrap();
            let recorded = reqwest::Url::parse(&format!(
                "http://broker{}",
                case["requestPath"].as_str().unwrap()
            ))
            .unwrap();
            let expected: Vec<(String, String)> = recorded.query_pairs().into_owned().collect();
            assert_eq!(query_pairs(&options), expected, "{}", case["requestPath"]);
        }
    }
    #[test]
    fn decisions_and_open_codes_are_rendered_without_reinterpreting_policy() {
        for (name, code) in [("allowed", 0), ("blocked", 1), ("unknown", 2)] {
            let raw = std::fs::read_to_string(format!(
                "tests/fixtures/pactling/explanations/{name}.json"
            ))
            .unwrap();
            let mut doc: serde_json::Value = serde_json::from_str(&raw).unwrap();
            assert_eq!(decision_code(&doc), Ok(code));
            let text = render_text(&doc);
            assert!(text.contains(name));
            for reason in doc["reasons"].as_array().unwrap() {
                assert!(text.contains(reason["code"].as_str().unwrap()));
            }
            for action in doc["nextActions"].as_array().unwrap() {
                assert!(text.contains(action["type"].as_str().unwrap()));
                for reference in action["evidenceRefs"].as_array().unwrap() {
                    let evidence = doc.pointer(reference.as_str().unwrap()).unwrap();
                    if let Some(href) = evidence.get("href") {
                        assert!(text.contains(href.as_str().unwrap()));
                    }
                }
            }
            doc["schemaVersion"] = serde_json::json!("1.9");
            doc["reasons"][0]["code"] = serde_json::json!("FUTURE_REASON");
            assert_eq!(decision_code(&doc), Ok(code));
            assert!(render_text(&doc).contains("FUTURE_REASON"));
        }
    }
    #[test]
    fn invalid_or_contradictory_decisions_are_request_errors() {
        let raw = include_str!("../tests/fixtures/pactling/explanations/allowed.json");
        let doc: serde_json::Value = serde_json::from_str(raw).unwrap();
        for (pointer, value) in [
            ("/schemaVersion", serde_json::json!("2.0")),
            ("/decision", serde_json::json!("maybe")),
            ("/compatibility/deployable", serde_json::Value::Null),
            ("/reasons", serde_json::Value::Null),
            ("/nextActions", serde_json::Value::Null),
        ] {
            let mut bad = doc.clone();
            *bad.pointer_mut(pointer).unwrap() = value;
            assert!(decision_code(&bad).is_err(), "{pointer}");
        }
    }
    fn pinned_args(base: &str) -> Explain {
        Explain::try_parse_from([
            "explain",
            "--pacticipant",
            "C & / ü",
            "--version",
            "1=2",
            "--to-environment",
            "test",
            "--broker-base-url",
            base,
        ])
        .unwrap()
    }
    #[test]
    fn endpoint_preserves_base_paths_and_encodes_values_without_leaking_credentials() {
        let url = endpoint(&pinned_args("https://example.com/broker%20path/")).unwrap();
        assert_eq!(
            url.path(),
            "/broker%20path/pactling/v1/can-i-deploy/explanation"
        );
        assert_eq!(url.query_pairs().next().unwrap().1, "C & / ü");
        assert_eq!(url.query_pairs().nth(1).unwrap().1, "1=2");
        for base in [
            "not a url",
            "ftp://host",
            "http://user@host",
            "http://:private-password@host",
            "http://host/?query=1",
            "http://host/#fragment",
        ] {
            let error = endpoint(&pinned_args(base)).unwrap_err();
            assert!(!error.contains("private-password"));
        }
    }
    #[test]
    fn text_includes_unknown_codes_and_all_reason_roles() {
        let mut doc: serde_json::Value = serde_json::from_str(include_str!(
            "../tests/fixtures/pactling/explanations/unknown.json"
        ))
        .unwrap();
        doc["reasons"].as_array_mut().unwrap().push(serde_json::json!({"role":"supporting","code":"SUPPORTING","message":"supporting-only-message"}));
        doc["reasons"].as_array_mut().unwrap().push(serde_json::json!({"role":"advisory","code":"ADVISORY","message":"advisory-only-message"}));
        doc["nextActions"][0]["type"] = serde_json::json!("FUTURE_ACTION");
        let text = render_text(&doc);
        assert!(text.contains("FUTURE_ACTION"));
        assert!(text.contains("[SUPPORTING]"));
        assert!(text.contains("[ADVISORY]"));
    }
}
