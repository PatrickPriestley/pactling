//! Webhooks: validation, descriptions, redaction and the create/update rules. Messages
//! and shapes follow the Phase 5 capture and the Ruby broker's `WebhookContract`.
use serde_json::{Map, Value, json};

use crate::config::Config;
use crate::db::{
    Db,
    webhooks::{Webhook, WebhookFields},
};
use crate::error::{Error, Result};

pub const EVENT_NAMES: [&str; 6] = [
    "contract_published",
    "contract_content_changed",
    "provider_verification_published",
    "provider_verification_succeeded",
    "provider_verification_failed",
    "contract_requiring_verification_published",
];
pub const DEFAULT_EVENT: &str = "contract_content_changed";
pub const REDACTED: &str = "**********";

/// `${pactbroker.something}` in a template string.
pub fn includes_parameter(value: &str) -> bool {
    parameter_regex().is_match(value)
}

fn parameter_regex() -> regex::Regex {
    regex::Regex::new(r"\$\{pactbroker\.[^}]+\}").expect("regex")
}

/// The URL with every template parameter replaced, for parsing (verified:
/// `Render.render_with_placeholder`).
pub fn with_placeholder(value: &str, placeholder: &str) -> String {
    parameter_regex()
        .replace_all(value, placeholder)
        .into_owned()
}

/// A scope by name or by label; labels never match a pacticipant in pactling.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Scope {
    pub name: Option<String>,
    pub label: Option<String>,
}

/// A validated create or update body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WebhookRequest {
    pub description: Option<String>,
    pub consumer: Option<Scope>,
    pub provider: Option<Scope>,
    pub enabled: bool,
    pub method: String,
    pub url: String,
    /// Header names as given; redacted values are resolved against the existing webhook.
    pub headers: Vec<(String, String)>,
    pub body: Option<Value>,
    pub username: Option<String>,
    pub password: Option<String>,
    pub events: Vec<String>,
}

pub fn valid_uuid(uuid: &str) -> bool {
    uuid.len() >= 16
        && uuid
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

fn scope_of(errors: &mut Map<String, Value>, key: &str, v: Option<&Value>) -> Option<Scope> {
    let v = v.filter(|v| !v.is_null())?;
    let name = v.get("name").and_then(Value::as_str).map(str::to_owned);
    let label = v.get("label").and_then(Value::as_str).map(str::to_owned);
    if name.is_some() && label.is_some() {
        errors.insert(
            format!("{key}.label"),
            json!(["name and label cannot be provided at the same time"]),
        );
    }
    Some(Scope { name, label })
}

impl WebhookRequest {
    /// Validates the body. Errors are keyed as the Ruby broker keys them.
    pub fn parse(body: &Value, uuid: Option<&str>, config: &Config) -> Result<WebhookRequest> {
        let mut errors: Map<String, Value> = Map::new();
        let request = match body.get("request") {
            None | Some(Value::Null) => {
                errors.insert("request".into(), json!(["is missing"]));
                None
            }
            Some(Value::Object(m)) if m.is_empty() => {
                errors.insert("request".into(), json!(["must be filled"]));
                None
            }
            Some(Value::Object(m)) => Some(m),
            Some(_) => {
                errors.insert("request".into(), json!(["must be a hash"]));
                None
            }
        };
        let consumer = scope_of(&mut errors, "consumer", body.get("consumer"));
        let provider = scope_of(&mut errors, "provider", body.get("provider"));
        let (mut method, mut url) = (String::new(), String::new());
        let (mut headers, mut body_value, mut username, mut password) = (vec![], None, None, None);
        if let Some(r) = request {
            match r.get("method").and_then(Value::as_str) {
                Some(m) if !m.is_empty() => {
                    method = m.to_owned();
                    let allowed = &config.webhook_http_method_whitelist;
                    if !allowed.iter().any(|a| a.eq_ignore_ascii_case(m)) {
                        let msg = if allowed.len() == 1 {
                            format!(
                                "must be {}. See /doc/webhooks#whitelist for more information.",
                                allowed[0]
                            )
                        } else {
                            format!(
                                "must be one of {}. See /doc/webhooks#whitelist for more information.",
                                allowed.join(", ")
                            )
                        };
                        errors.insert("request.method".into(), json!([msg]));
                    }
                }
                _ => {
                    errors.insert("request.method".into(), json!(["is missing"]));
                }
            }
            match r.get("url").and_then(Value::as_str) {
                Some(u) if !u.is_empty() => {
                    url = u.to_owned();
                    if let Some(msg) = url_error(u, config) {
                        errors.insert("request.url".into(), json!([msg]));
                    }
                }
                _ => {
                    errors.insert("request.url".into(), json!(["is missing"]));
                }
            }
            if let Some(Value::Object(h)) = r.get("headers") {
                for (k, v) in h {
                    let text = match v {
                        Value::String(s) => s.clone(),
                        other => other.to_string(),
                    };
                    headers.push((k.clone(), text));
                }
            }
            body_value = r.get("body").filter(|b| !b.is_null()).cloned();
            username = r.get("username").and_then(Value::as_str).map(str::to_owned);
            password = r.get("password").and_then(Value::as_str).map(str::to_owned);
        }
        let events = match body.get("events") {
            None | Some(Value::Null) => vec![DEFAULT_EVENT.to_owned()],
            Some(Value::Array(a)) if a.is_empty() => {
                errors.insert("events".into(), json!(["size cannot be less than 1"]));
                vec![]
            }
            Some(Value::Array(a)) => {
                let mut names = vec![];
                for (i, e) in a.iter().enumerate() {
                    match e.get("name").and_then(Value::as_str) {
                        Some(n) if EVENT_NAMES.contains(&n) => names.push(n.to_owned()),
                        _ => {
                            errors.entry("events").or_insert_with(|| {
                                json!([format!(
                                    "name must be one of: {} (at index {i})",
                                    EVENT_NAMES.join(", ")
                                )])
                            });
                        }
                    }
                }
                names
            }
            Some(_) => {
                errors.insert("events".into(), json!(["must be an array"]));
                vec![]
            }
        };
        let enabled = match body.get("enabled") {
            None | Some(Value::Null) => true,
            Some(Value::Bool(b)) => *b,
            Some(_) => {
                errors.insert("enabled".into(), json!(["must be boolean"]));
                true
            }
        };
        if let Some(u) = uuid
            && !valid_uuid(u)
        {
            errors.insert("uuid".into(), json!(["The UUID can only contain the characters A-Z, a-z, 0-9, _ and -, and must be 16 or more characters."]));
        }
        if !errors.is_empty() {
            return Err(Error::Validation(Value::Object(errors)));
        }
        Ok(WebhookRequest {
            description: body
                .get("description")
                .and_then(Value::as_str)
                .map(str::to_owned),
            consumer,
            provider,
            enabled,
            method,
            url,
            headers,
            body: body_value,
            username,
            password,
            events,
        })
    }
}

fn url_error(url: &str, config: &Config) -> Option<String> {
    let parsed = reqwest::Url::parse(&with_placeholder(url, "placeholder")).ok();
    let Some(parsed) = parsed.filter(|p| p.host_str().is_some()) else {
        return Some("is not a valid URL eg. http://example.org".into());
    };
    let other = reqwest::Url::parse(&with_placeholder(url, "differentplaceholder")).ok();
    if other.as_ref().and_then(|o| o.host_str().map(str::to_owned))
        != parsed.host_str().map(str::to_owned)
    {
        return Some("cannot have a template parameter in the host".into());
    }
    if !config
        .webhook_scheme_whitelist
        .iter()
        .any(|s| s.eq_ignore_ascii_case(parsed.scheme()))
    {
        return Some(format!(
            "scheme must be {}. See /doc/webhooks#whitelist for more information.",
            config.webhook_scheme_whitelist.join(", ")
        ));
    }
    if !config.webhook_host_whitelist.is_empty()
        && !host_allowed(
            parsed.host_str().unwrap_or_default(),
            &config.webhook_host_whitelist,
        )
    {
        return Some(format!(
            "host must be in the whitelist {}. See /doc/webhooks#whitelist for more information.",
            config
                .webhook_host_whitelist
                .iter()
                .map(|h| format!("{h:?}"))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    None
}

/// Exact host names, or `/regex/` entries (the Ruby broker also accepts CIDR ranges,
/// which pactling does not).
pub fn host_allowed(host: &str, whitelist: &[String]) -> bool {
    whitelist.iter().any(|entry| {
        if let Some(pattern) = entry.strip_prefix('/').and_then(|e| e.strip_suffix('/')) {
            regex::Regex::new(pattern)
                .map(|r| r.is_match(host))
                .unwrap_or(false)
        } else {
            entry.eq_ignore_ascii_case(host)
        }
    })
}

/// "POST host" (verified: `WebhookRequestTemplate#description`).
pub fn request_description(w: &Webhook) -> String {
    let host = reqwest::Url::parse(&with_placeholder(&w.url, "placeholder"))
        .ok()
        .and_then(|u| u.host_str().map(str::to_owned))
        .unwrap_or_default();
    format!("{} {host}", w.method.to_uppercase())
}

pub fn display_description(w: &Webhook) -> String {
    match &w.description {
        Some(d) if !d.trim().is_empty() => d.clone(),
        _ => request_description(w),
    }
}

/// The name of a webhook in publish notices: its description if set, else its uuid
/// (verified in the capture's publish output).
pub fn notice_name(w: &Webhook) -> String {
    match &w.description {
        Some(d) if !d.trim().is_empty() => format!("{d:?}"),
        _ => w.uuid.clone(),
    }
}

pub fn consumer_name(w: &Webhook) -> Option<String> {
    w.consumer_name.clone().or_else(|| {
        w.consumer_label.as_ref().map(|l| {
            format!(
                "{}labeled '{l}'",
                if w.provider_id.is_some() || w.provider_label.is_some() {
                    "consumers "
                } else {
                    ""
                }
            )
        })
    })
}

pub fn provider_name(w: &Webhook) -> Option<String> {
    w.provider_name.clone().or_else(|| {
        w.provider_label.as_ref().map(|l| {
            format!(
                "{}labeled '{l}'",
                if w.consumer_id.is_some() || w.consumer_label.is_some() {
                    "providers "
                } else {
                    ""
                }
            )
        })
    })
}

pub fn scope_description(w: &Webhook) -> String {
    match (consumer_name(w), provider_name(w)) {
        (Some(c), Some(p)) => format!("A webhook for the pact between {c} and {p}"),
        (None, Some(p)) => format!("A webhook for all pacts with provider {p}"),
        (Some(c), None) => format!("A webhook for all pacts with consumer {c}"),
        (None, None) => "A webhook for all pacts".into(),
    }
}

pub fn headers_of(w: &Webhook) -> Vec<(String, String)> {
    w.headers
        .as_deref()
        .and_then(|h| serde_json::from_str::<Value>(h).ok())
        .and_then(|v| match v {
            Value::Object(m) => Some(
                m.into_iter()
                    .map(|(k, v)| {
                        (
                            k,
                            v.as_str()
                                .map(str::to_owned)
                                .unwrap_or_else(|| v.to_string()),
                        )
                    })
                    .collect(),
            ),
            _ => None,
        })
        .unwrap_or_default()
}

/// Header names are lower-cased; values of headers named like authorization or token
/// are hidden unless they hold a template parameter (verified: `redacted_headers`).
pub fn redacted_headers(w: &Webhook) -> Vec<(String, String)> {
    headers_of(w)
        .into_iter()
        .map(|(k, v)| {
            let lower = k.to_lowercase();
            let redact = (lower.contains("authorization") || lower.contains("token"))
                && !includes_parameter(&v);
            (lower, if redact { REDACTED.to_owned() } else { v })
        })
        .collect()
}

pub fn display_password(w: &Webhook) -> Option<String> {
    w.password.as_ref().map(|p| {
        if includes_parameter(p) {
            p.clone()
        } else {
            REDACTED.to_owned()
        }
    })
}

pub fn body_value(w: &Webhook) -> Option<Value> {
    w.body.as_deref().and_then(|b| serde_json::from_str(b).ok())
}

/// A fresh uuid as the Ruby broker makes them: 22 url-safe base64 characters.
pub fn new_uuid() -> String {
    use base64::Engine;
    let bytes = uuid::Uuid::new_v4();
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes.as_bytes())
}

async fn resolve_scope(
    db: &Db,
    key: &str,
    scope: &Option<Scope>,
    errors: &mut Map<String, Value>,
) -> Result<(Option<i64>, Option<String>)> {
    let Some(s) = scope else {
        return Ok((None, None));
    };
    let id = match &s.name {
        Some(name) => match db.find_pacticipant(name).await? {
            Some(p) => Some(p.id),
            None => {
                errors.insert(
                    format!("{key}.name"),
                    json!(["does not match an existing pacticipant"]),
                );
                None
            }
        },
        None => None,
    };
    Ok((id, s.label.clone()))
}

async fn fields(
    db: &Db,
    req: &WebhookRequest,
    existing: Option<&Webhook>,
) -> Result<WebhookFields> {
    let mut errors = Map::new();
    let (consumer_id, consumer_label) =
        resolve_scope(db, "consumer", &req.consumer, &mut errors).await?;
    let (provider_id, provider_label) =
        resolve_scope(db, "provider", &req.provider, &mut errors).await?;
    if !errors.is_empty() {
        return Err(Error::Validation(Value::Object(errors)));
    }
    // Verified (`maintain_redacted_params`): a starred password or header value on update
    // keeps the stored one.
    let is_starred = |v: &str| !v.is_empty() && v.bytes().all(|b| b == b'*');
    let password = match (&req.password, existing) {
        (Some(p), Some(e)) if is_starred(p) => e.password.clone(),
        (None, Some(e)) => e.password.clone(),
        (p, _) => p.clone(),
    };
    let existing_headers = existing.map(headers_of).unwrap_or_default();
    let headers: Map<String, Value> = req
        .headers
        .iter()
        .map(|(k, v)| {
            let value = if is_starred(v) {
                existing_headers
                    .iter()
                    .find(|(ek, _)| ek.eq_ignore_ascii_case(k))
                    .map(|(_, ev)| ev.clone())
                    .unwrap_or_else(|| v.clone())
            } else {
                v.clone()
            };
            (k.clone(), Value::String(value))
        })
        .collect();
    Ok(WebhookFields {
        description: req.description.clone(),
        consumer_id,
        consumer_label,
        provider_id,
        provider_label,
        enabled: req.enabled,
        method: req.method.clone(),
        url: req.url.clone(),
        headers: if headers.is_empty() {
            None
        } else {
            Some(Value::Object(headers).to_string())
        },
        body: req.body.as_ref().map(|b| b.to_string()),
        username: req.username.clone(),
        password,
        events: req.events.clone(),
    })
}

pub async fn create(db: &Db, uuid: Option<&str>, req: &WebhookRequest) -> Result<Webhook> {
    let f = fields(db, req, None).await?;
    let uuid = uuid.map(str::to_owned).unwrap_or_else(new_uuid);
    Ok(db.create_webhook(&uuid, &f).await?)
}

pub async fn update(db: &Db, existing: &Webhook, req: &WebhookRequest) -> Result<Webhook> {
    let f = fields(db, req, Some(existing)).await?;
    Ok(db.update_webhook(existing.id, &f).await?)
}

pub async fn find(db: &Db, uuid: &str) -> Result<Webhook> {
    db.find_webhook_by_uuid(uuid).await?.ok_or(Error::NoRoute)
}

pub async fn delete(db: &Db, uuid: &str) -> Result<()> {
    let w = find(db, uuid).await?;
    db.delete_webhook(w.id).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> Config {
        let mut c = Config::in_memory();
        c.webhook_scheme_whitelist = vec!["http".into()];
        c
    }

    #[test]
    fn validation_messages_match_the_capture() {
        let err = |body: Value| match WebhookRequest::parse(&body, None, &config()) {
            Err(Error::Validation(v)) => v,
            other => panic!("{other:?}"),
        };
        assert_eq!(err(json!({})), json!({"request": ["is missing"]}));
        assert_eq!(
            err(json!({"request": {}})),
            json!({"request": ["must be filled"]})
        );
        assert_eq!(
            err(json!({"request": {"method": "GET", "url": "http://x/hook"}})),
            json!({"request.method": ["must be POST. See /doc/webhooks#whitelist for more information."]})
        );
        assert_eq!(
            err(json!({"request": {"method": "POST", "url": "https://example.com/hook"}})),
            json!({"request.url": ["scheme must be http. See /doc/webhooks#whitelist for more information."]})
        );
        assert_eq!(
            err(json!({"request": {"method": "POST", "url": "nope"}})),
            json!({"request.url": ["is not a valid URL eg. http://example.org"]})
        );
        assert_eq!(
            err(
                json!({"request": {"method": "POST", "url": "http://${pactbroker.consumerName}.example.com/hook"}})
            ),
            json!({"request.url": ["cannot have a template parameter in the host"]})
        );
        assert_eq!(
            err(
                json!({"request": {"method": "POST", "url": "http://x/hook"}, "events": [{"name": "bogus"}]})
            ),
            json!({"events": ["name must be one of: contract_published, contract_content_changed, provider_verification_published, provider_verification_succeeded, provider_verification_failed, contract_requiring_verification_published (at index 0)"]})
        );
        assert_eq!(
            err(json!({"request": {"method": "POST", "url": "http://x/hook"}, "events": []})),
            json!({"events": ["size cannot be less than 1"]})
        );
        assert_eq!(
            err(
                json!({"consumer": {"name": "C", "label": "x"}, "request": {"method": "POST", "url": "http://x/hook"}})
            ),
            json!({"consumer.label": ["name and label cannot be provided at the same time"]})
        );
        let ok = WebhookRequest::parse(
            &json!({"request": {"method": "POST", "url": "http://x/hook"}}),
            Some("pactling-capture-webhook-1"),
            &config(),
        )
        .unwrap();
        assert_eq!(ok.events, vec![DEFAULT_EVENT.to_owned()]);
        assert!(ok.enabled);
        assert!(matches!(
            WebhookRequest::parse(&json!({"request": {"method": "POST", "url": "http://x/hook"}}), Some("short"), &config()),
            Err(Error::Validation(v)) if v["uuid"][0].as_str().unwrap().starts_with("The UUID can only contain")
        ));
    }

    #[test]
    fn uuids_are_22_urlsafe_characters() {
        let u = new_uuid();
        assert_eq!(u.len(), 22);
        assert!(valid_uuid(&u));
    }

    #[test]
    fn host_whitelist_accepts_names_and_regexes() {
        assert!(host_allowed("ci.example.com", &["ci.example.com".into()]));
        assert!(host_allowed(
            "ci.example.com",
            &["/.*\\.example\\.com/".into()]
        ));
        assert!(!host_allowed("evil.com", &["ci.example.com".into()]));
    }
}
