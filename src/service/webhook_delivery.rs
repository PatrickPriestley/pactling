//! Building, sending and logging a webhook request. The request shape, template
//! parameters and log lines follow the Phase 5 capture and the Ruby broker's
//! `WebhookRequestTemplate`, `PactAndVerificationParameters` and `WebhookRequestLogger`.
use serde_json::{Map, Value, json};

use crate::config::Config;
use crate::db::{Db, pacts::PactPublication, verifications::Verification, webhooks::Webhook};
use crate::error::Result;
use crate::service::webhooks::{self as webhooks, WebhookRequest, includes_parameter};

pub const USER_AGENT: &str = concat!("Pactling/", env!("CARGO_PKG_VERSION"));

/// The request template of a saved or unsaved webhook.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Template {
    pub uuid: Option<String>,
    pub consumer_name: Option<String>,
    pub provider_name: Option<String>,
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<Value>,
    pub username: Option<String>,
    pub password: Option<String>,
}

impl Template {
    pub fn of_webhook(w: &Webhook) -> Template {
        Template {
            uuid: Some(w.uuid.clone()),
            consumer_name: w.consumer_name.clone(),
            provider_name: w.provider_name.clone(),
            method: w.method.clone(),
            url: w.url.clone(),
            headers: webhooks::headers_of(w),
            body: webhooks::body_value(w),
            username: w.username.clone(),
            password: w.password.clone(),
        }
    }

    pub fn of_request(r: &WebhookRequest) -> Template {
        Template {
            uuid: None,
            consumer_name: r.consumer.as_ref().and_then(|c| c.name.clone()),
            provider_name: r.provider.as_ref().and_then(|p| p.name.clone()),
            method: r.method.clone(),
            url: r.url.clone(),
            headers: r.headers.clone(),
            body: r.body.clone(),
            username: r.username.clone(),
            password: r.password.clone(),
        }
    }
}

/// The twenty `${pactbroker.*}` parameters, in the Ruby broker's order. `context` is the
/// triggered webhook's event context (verified keys: base_url, event_name,
/// consumer_version_number, consumer_version_tags, consumer_version_branch, build_url,
/// provider_version_number, provider_version_branch, provider_version_descriptions,
/// provider_version_tags, currently_deployed_provider_version_number).
pub async fn parameters(
    db: &Db,
    pact: Option<&PactPublication>,
    verification: Option<&Verification>,
    context: &Map<String, Value>,
) -> Result<Vec<(String, String)>> {
    let base = context
        .get("base_url")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let string = |key: &str| context.get(key).and_then(Value::as_str).map(str::to_owned);
    let list = |key: &str| {
        context.get(key).and_then(Value::as_array).map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        })
    };
    let pact_url = match pact {
        Some(p) => {
            let tags = db.tags_for_version(p.consumer_version_id).await?;
            let mut meta = format!("cvn={}", form_escape(&p.consumer_version_number));
            for t in &tags {
                meta.push_str(&format!("&cvt%5B%5D={}", form_escape(&t.name)));
            }
            meta.push_str("&w=true");
            format!(
                "{base}/pacts/provider/{}/consumer/{}/pact-version/{}/metadata/{}",
                crate::http::hal::encode(&p.provider_name),
                crate::http::hal::encode(&p.consumer_name),
                p.sha,
                base64_url(&meta)
            )
        }
        None => String::new(),
    };
    let verification_url = match (pact, verification) {
        (Some(p), Some(v)) => format!(
            "{base}/pacts/provider/{}/consumer/{}/pact-version/{}/verification-results/{}",
            crate::http::hal::encode(&p.provider_name),
            crate::http::hal::encode(&p.consumer_name),
            p.sha,
            v.number
        ),
        _ => String::new(),
    };
    let consumer_version_number = match string("consumer_version_number") {
        Some(n) => n,
        None => pact
            .map(|p| p.consumer_version_number.clone())
            .unwrap_or_default(),
    };
    let consumer_version_tags = match list("consumer_version_tags") {
        Some(t) => t,
        None => match pact {
            Some(p) => db
                .tags_for_version(p.consumer_version_id)
                .await?
                .into_iter()
                .map(|t| t.name)
                .collect::<Vec<_>>()
                .join(", "),
            None => String::new(),
        },
    };
    let consumer_version_branch = if context.contains_key("consumer_version_branch") {
        string("consumer_version_branch").unwrap_or_default()
    } else {
        match pact {
            Some(p) => db
                .branch_versions_for_version(p.consumer_version_id)
                .await?
                .last()
                .map(|b| b.branch_name.clone())
                .unwrap_or_default(),
            None => String::new(),
        }
    };
    let provider_version = match verification {
        Some(v) => db.find_version_by_id(v.provider_version_id).await?,
        None => None,
    };
    let provider_version_tags = match list("provider_version_tags") {
        Some(t) => t,
        None => match &provider_version {
            Some(pv) => db
                .tags_for_version(pv.id)
                .await?
                .into_iter()
                .map(|t| t.name)
                .collect::<Vec<_>>()
                .join(", "),
            None => String::new(),
        },
    };
    let provider_version_number = match string("provider_version_number") {
        Some(n) => n,
        None => verification
            .map(|v| v.provider_version_number.clone())
            .unwrap_or_default(),
    };
    let provider_version_branch = if context.contains_key("provider_version_branch") {
        string("provider_version_branch").unwrap_or_default()
    } else {
        match &provider_version {
            Some(pv) => db
                .branch_versions_for_version(pv.id)
                .await?
                .last()
                .map(|b| b.branch_name.clone())
                .unwrap_or_default(),
            None => String::new(),
        }
    };
    let status = |success: &str, failure: &str, none: &str| match verification {
        Some(v) if v.success => success.to_owned(),
        Some(_) => failure.to_owned(),
        None => none.to_owned(),
    };
    Ok(vec![
        ("pactbroker.pactUrl".into(), pact_url),
        ("pactbroker.verificationResultUrl".into(), verification_url),
        (
            "pactbroker.consumerVersionNumber".into(),
            consumer_version_number,
        ),
        (
            "pactbroker.providerVersionNumber".into(),
            provider_version_number,
        ),
        (
            "pactbroker.providerVersionTags".into(),
            provider_version_tags,
        ),
        (
            "pactbroker.providerVersionBranch".into(),
            provider_version_branch,
        ),
        (
            "pactbroker.providerVersionDescriptions".into(),
            list("provider_version_descriptions").unwrap_or_default(),
        ),
        (
            "pactbroker.consumerVersionTags".into(),
            consumer_version_tags,
        ),
        (
            "pactbroker.consumerVersionBranch".into(),
            consumer_version_branch,
        ),
        (
            "pactbroker.consumerName".into(),
            pact.map(|p| p.consumer_name.clone()).unwrap_or_default(),
        ),
        (
            "pactbroker.providerName".into(),
            pact.map(|p| p.provider_name.clone()).unwrap_or_default(),
        ),
        (
            "pactbroker.githubVerificationStatus".into(),
            status("success", "failure", "pending"),
        ),
        (
            "pactbroker.bitbucketVerificationStatus".into(),
            status("SUCCESSFUL", "FAILED", "INPROGRESS"),
        ),
        (
            "pactbroker.azureDevOpsVerificationStatus".into(),
            status("succeeded", "failed", "pending"),
        ),
        (
            "pactbroker.gitlabVerificationStatus".into(),
            status("success", "failed", "pending"),
        ),
        ("pactbroker.consumerLabels".into(), String::new()),
        ("pactbroker.providerLabels".into(), String::new()),
        (
            "pactbroker.eventName".into(),
            string("event_name").unwrap_or_default(),
        ),
        (
            "pactbroker.buildUrl".into(),
            string("build_url").unwrap_or_default(),
        ),
        (
            "pactbroker.currentlyDeployedProviderVersionNumber".into(),
            string("currently_deployed_provider_version_number").unwrap_or_default(),
        ),
    ])
}

fn base64_url(s: &str) -> String {
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(s)
}

/// Ruby's `CGI.escape`: form encoding, spaces as `+`.
pub fn form_escape(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'*' => {
                out.push(b as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Replaces every `${key}` with its value (verified: `Render.call`).
pub fn render(template: &str, params: &[(String, String)], escape: bool) -> String {
    let mut out = template.to_owned();
    for (k, v) in params {
        let value = if escape { form_escape(v) } else { v.clone() };
        out = out.replace(&format!("${{{k}}}"), &value);
    }
    out
}

/// The request as it goes on the wire; headers in the order the Ruby broker logs them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BuiltRequest {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

pub fn build(t: &Template, params: &[(String, String)]) -> BuiltRequest {
    let url = render(&t.url, params, true);
    let body = match &t.body {
        Some(Value::String(s)) => render(s, params, false),
        Some(other) => render(&other.to_string(), params, false),
        None => "null".to_owned(),
    };
    let custom: Vec<(String, String)> = t
        .headers
        .iter()
        .map(|(k, v)| (k.to_lowercase(), render(v, params, false)))
        .collect();
    let mut headers: Vec<(String, String)> = vec![];
    let accept = custom
        .iter()
        .find(|(k, _)| k == "accept")
        .map(|(_, v)| v.clone())
        .unwrap_or_else(|| "*/*".to_owned());
    headers.push(("accept".into(), accept));
    headers.push(("user-agent".into(), USER_AGENT.into()));
    for (k, v) in &custom {
        if k != "accept" {
            headers.push((k.clone(), v.clone()));
        }
    }
    if let Some(u) = t.username.as_deref().filter(|u| !u.is_empty()) {
        use base64::Engine;
        let user = render(u, params, false);
        let pass = t
            .password
            .as_deref()
            .map(|p| render(p, params, false))
            .unwrap_or_default();
        let value = base64::engine::general_purpose::STANDARD.encode(format!("{user}:{pass}"));
        headers.push(("authorization".into(), format!("Basic {value}")));
    }
    if let Ok(parsed) = reqwest::Url::parse(&url) {
        let host = match (parsed.host_str(), parsed.port()) {
            (Some(h), Some(p)) => format!("{h}:{p}"),
            (Some(h), None) => h.to_owned(),
            _ => String::new(),
        };
        headers.push(("host".into(), host));
    }
    headers.push(("content-length".into(), body.len().to_string()));
    if !custom.iter().any(|(k, _)| k == "content-type") {
        headers.push((
            "content-type".into(),
            "application/x-www-form-urlencoded".into(),
        ));
    }
    BuiltRequest {
        method: t.method.to_uppercase(),
        url,
        headers,
        body,
    }
}

pub fn redacted(headers: &[(String, String)]) -> Vec<(String, String)> {
    headers
        .iter()
        .map(|(k, v)| {
            let hide =
                (k.contains("authorization") || k.contains("token")) && !includes_parameter(v);
            (
                k.clone(),
                if hide {
                    webhooks::REDACTED.to_owned()
                } else {
                    v.clone()
                },
            )
        })
        .collect()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResponseInfo {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExecutionResult {
    pub request: BuiltRequest,
    pub response: Option<ResponseInfo>,
    pub error: Option<String>,
    pub success: bool,
    pub logs: String,
}

/// What an execution logs at its end.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Messages {
    pub success: Option<String>,
    pub failure: Option<String>,
}

fn log_time() -> String {
    chrono::Utc::now()
        .format("[%Y-%m-%dT%H:%M:%SZ]")
        .to_string()
}

pub fn hidden_response_message(base_url: &str) -> String {
    format!(
        "For security purposes, the response details are not logged. To enable response logging, configure the webhook_host_whitelist property. See {base_url}/doc/webhooks#whitelist for more information."
    )
}

/// Sends the request and returns the result with the Ruby broker's log text.
pub async fn execute(
    config: &Config,
    uuid: Option<&str>,
    built: &BuiltRequest,
    context: &Map<String, Value>,
    messages: &Messages,
) -> ExecutionResult {
    let base_url = context
        .get("base_url")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let show_response = !config.webhook_hosts().is_empty();
    let mut logs = String::new();
    let mut line = |level: &str, text: &str| {
        logs.push_str(&format!("{} {level}: {text}\n", log_time()));
    };
    line(
        "DEBUG",
        &format!("Webhook context {}", Value::Object(context.clone())),
    );
    line("INFO", &format!("HTTP/1.1 {} {}", built.method, built.url));
    for (k, v) in redacted(&built.headers) {
        line("INFO", &format!("{k}: {v}"));
    }
    line("INFO", &built.body);
    let client = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(std::time::Duration::from_secs(15))
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .expect("client");
    let mut req = client.request(
        built.method.parse().unwrap_or(reqwest::Method::POST),
        &built.url,
    );
    for (k, v) in &built.headers {
        if k == "host" || k == "content-length" || k == "user-agent" {
            continue;
        }
        req = req.header(k.as_str(), v.as_str());
    }
    let sent = req.body(built.body.clone()).send().await;
    let (response, error) = match sent {
        Ok(r) => {
            let status = r.status().as_u16();
            let headers = r
                .headers()
                .iter()
                .map(|(k, v)| (k.as_str().to_owned(), v.to_str().unwrap_or("").to_owned()))
                .collect();
            let body = r.text().await.unwrap_or_default();
            (
                Some(ResponseInfo {
                    status,
                    headers,
                    body,
                }),
                None,
            )
        }
        Err(e) => (None, Some(e.to_string())),
    };
    let success = response
        .as_ref()
        .is_some_and(|r| config.webhook_http_code_success.contains(&r.status));
    let uuid_text = uuid.unwrap_or_default();
    if let Some(r) = &response {
        if show_response {
            line(
                "INFO",
                &format!(
                    "HTTP/1.1 {} {}",
                    r.status,
                    reqwest::StatusCode::from_u16(r.status)
                        .ok()
                        .and_then(|s| s.canonical_reason())
                        .unwrap_or("")
                ),
            );
            for (k, v) in &r.headers {
                line("INFO", &format!("{k}: {v}"));
            }
            if !r.body.is_empty() {
                line("INFO", &r.body);
            }
        } else {
            line("INFO", &hidden_response_message(&base_url));
        }
    }
    if let Some(e) = &error {
        if show_response {
            line("ERROR", &format!("Error executing webhook {uuid_text} {e}"));
        } else {
            line(
                "ERROR",
                &format!(
                    "Error executing webhook {uuid_text}. {}",
                    hidden_response_message(&base_url)
                ),
            );
        }
    }
    if success {
        if let Some(m) = &messages.success {
            line("INFO", m);
        }
    } else if let Some(m) = &messages.failure {
        line("INFO", m);
    }
    ExecutionResult {
        request: built.clone(),
        response,
        error,
        success,
        logs,
    }
}

/// The latest pact a webhook's scope matches (verified: `search_for_latest_pact`), newest
/// consumer version first.
pub async fn latest_pact_for(
    db: &Db,
    consumer: Option<&str>,
    provider: Option<&str>,
) -> Result<Option<PactPublication>> {
    let mut pacts: Vec<PactPublication> = db
        .latest_pacts()
        .await?
        .into_iter()
        .filter(|p| {
            consumer.is_none_or(|c| p.consumer_name == c)
                && provider.is_none_or(|pr| p.provider_name == pr)
        })
        .collect();
    pacts.sort_by_key(|p| std::cmp::Reverse(p.id));
    Ok(pacts.into_iter().next())
}

/// `POST /webhooks/{uuid}/execute` and `/webhooks/execute`: the latest matching pact and
/// its latest verification, event name `test` (verified: `test_execution`).
pub async fn test_execution(
    db: &Db,
    config: &Config,
    t: &Template,
    base_url: &str,
) -> Result<ExecutionResult> {
    let pact = latest_pact_for(db, t.consumer_name.as_deref(), t.provider_name.as_deref()).await?;
    let verification = match &pact {
        Some(p) => {
            db.latest_verification_for_pact_version(p.pact_version_id)
                .await?
        }
        None => None,
    };
    let mut context = Map::new();
    context.insert("base_url".into(), json!(base_url));
    context.insert("event_name".into(), json!("test"));
    let params = parameters(db, pact.as_ref(), verification.as_ref(), &context).await?;
    let built = build(t, &params);
    let messages = Messages {
        success: None,
        failure: Some("Webhook execution failed".into()),
    };
    Ok(execute(config, t.uuid.as_deref(), &built, &context, &messages).await)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_parameters_are_form_escaped_and_body_parameters_are_not() {
        let params = vec![("pactbroker.consumerName".to_owned(), "My App".to_owned())];
        assert_eq!(
            render("http://x/${pactbroker.consumerName}", &params, true),
            "http://x/My+App"
        );
        assert_eq!(
            render("{\"c\":\"${pactbroker.consumerName}\"}", &params, false),
            "{\"c\":\"My App\"}"
        );
    }

    #[test]
    fn requests_carry_the_ruby_brokers_default_headers() {
        let t = Template {
            uuid: None,
            consumer_name: None,
            provider_name: None,
            method: "post".into(),
            url: "http://example.com:8080/hook".into(),
            headers: vec![("X-Token".into(), "abc".into())],
            body: None,
            username: Some("u".into()),
            password: Some("p".into()),
        };
        let b = build(&t, &[]);
        assert_eq!(b.method, "POST");
        assert_eq!(b.body, "null");
        let names: Vec<&str> = b.headers.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(
            names,
            [
                "accept",
                "user-agent",
                "x-token",
                "authorization",
                "host",
                "content-length",
                "content-type"
            ]
        );
        assert_eq!(b.headers[4].1, "example.com:8080");
        assert_eq!(b.headers[6].1, "application/x-www-form-urlencoded");
        assert_eq!(redacted(&b.headers)[2].1, "**********");
        assert_eq!(redacted(&b.headers)[3].1, "**********");
    }
}
