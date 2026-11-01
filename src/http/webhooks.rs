//! `/webhooks`, the scoped webhook collections, triggered webhooks, their logs and the
//! pact webhook status. Shapes follow the Phase 5 capture (tests/webhooks.rs).
use axum::{
    body::Bytes,
    extract::{Path, State},
    http::{Method, StatusCode},
    response::{IntoResponse, Response},
};
use serde_json::{Map, Value, json};

use crate::app::AppState;
use crate::db::{
    pacticipants::Pacticipant,
    webhooks::{TriggeredWebhook, Webhook},
};
use crate::error::{Error, Result};
use crate::http::hal::{Hal, Links, encode};
use crate::http::method;
use crate::http::pacts::pact_href;
use crate::http::versions::pacticipant_href;
use crate::service::webhooks::{self as service, WebhookRequest};

const COLLECTION_ALLOWED: &str = "GET, POST, OPTIONS";
const WEBHOOK_ALLOWED: &str = "GET, PUT, DELETE, OPTIONS";
const EXECUTE_ALLOWED: &str = "POST, OPTIONS";
const READ_ALLOWED: &str = "GET, OPTIONS";

fn parse_body(body: &Bytes) -> Result<Value> {
    if body.is_empty() {
        return Ok(json!({}));
    }
    serde_json::from_slice(body).map_err(|e| Error::BadRequest(format!("JSON::ParserError - {e}")))
}

pub fn webhook_href(l: &Links, uuid: &str) -> String {
    l.href(&format!("/webhooks/{}", encode(uuid)))
}

pub fn triggered_webhook_logs_href(l: &Links, uuid: &str) -> String {
    l.href(&format!("/triggered-webhooks/{uuid}/logs"))
}

/// The webhook document (verified: `WebhookDecorator`).
pub fn webhook_document(l: &Links, w: &Webhook) -> Value {
    let display = service::display_description(w);
    let mut doc = Map::new();
    doc.insert("uuid".into(), json!(w.uuid));
    doc.insert("description".into(), json!(display));
    for (key, name, label) in [
        ("consumer", &w.consumer_name, &w.consumer_label),
        ("provider", &w.provider_name, &w.provider_label),
    ] {
        if let Some(n) = name {
            doc.insert(key.into(), json!({ "name": n }));
        } else if let Some(lb) = label {
            doc.insert(key.into(), json!({ "label": lb }));
        }
    }
    doc.insert("enabled".into(), json!(w.enabled));
    let mut request = Map::new();
    request.insert("method".into(), json!(w.method));
    request.insert("url".into(), json!(w.url));
    let headers = service::redacted_headers(w);
    if !headers.is_empty() {
        request.insert(
            "headers".into(),
            Value::Object(headers.into_iter().map(|(k, v)| (k, json!(v))).collect()),
        );
    }
    if let Some(b) = service::body_value(w) {
        request.insert("body".into(), b);
    }
    if let Some(u) = &w.username {
        request.insert("username".into(), json!(u));
    }
    if let Some(p) = service::display_password(w) {
        request.insert("password".into(), json!(p));
    }
    doc.insert("request".into(), Value::Object(request));
    doc.insert(
        "events".into(),
        json!(
            w.events
                .iter()
                .map(|e| json!({ "name": e }))
                .collect::<Vec<_>>()
        ),
    );
    if let Some(u) = &w.updated_at {
        doc.insert("updatedAt".into(), json!(u));
    }
    doc.insert("createdAt".into(), json!(w.created_at));
    let mut links = Map::new();
    links.insert(
        "self".into(),
        json!({ "title": display, "href": webhook_href(l, &w.uuid) }),
    );
    links.insert("pb:execute".into(), json!({ "title": "Test the execution of the webhook with the latest matching pact or verification by sending a POST request to this URL", "href": format!("{}/execute", webhook_href(l, &w.uuid)) }));
    if let Some(c) = &w.consumer_name {
        links.insert(
            "pb:consumer".into(),
            json!({ "title": "Consumer", "name": c, "href": pacticipant_href(l, c) }),
        );
    }
    if let Some(lb) = &w.consumer_label {
        links.insert("pb:consumers".into(), json!({ "title": "Consumers by label", "name": lb, "href": l.href(&format!("/pacticipants/label/{}", encode(lb))) }));
    }
    if let Some(p) = &w.provider_name {
        links.insert(
            "pb:provider".into(),
            json!({ "title": "Provider", "name": p, "href": pacticipant_href(l, p) }),
        );
    }
    if let Some(lb) = &w.provider_label {
        links.insert("pb:providers".into(), json!({ "title": "Providers by label", "name": lb, "href": l.href(&format!("/pacticipants/label/{}", encode(lb))) }));
    }
    if let (Some(c), Some(p)) = (&w.consumer_name, &w.provider_name) {
        links.insert("pb:pact-webhooks".into(), json!({ "title": format!("All webhooks for consumer {c} and provider {p}"), "href": l.href(&format!("/webhooks/provider/{}/consumer/{}", encode(p), encode(c))) }));
    }
    links.insert(
        "pb:webhooks".into(),
        json!({ "title": "All webhooks", "href": l.href("/webhooks") }),
    );
    doc.insert("_links".into(), Value::Object(links));
    Value::Object(doc)
}

fn collection_document(l: &Links, title: &str, path: &str, webhooks: &[Webhook]) -> Value {
    let entries: Vec<Value> = webhooks
        .iter()
        .map(|w| json!({ "title": service::scope_description(w), "name": service::display_description(w), "href": webhook_href(l, &w.uuid) }))
        .collect();
    json!({
        "_links": {
            "self": { "title": title, "href": l.href(path) },
            "pb:create": { "title": "POST to create a webhook", "href": l.href(path) },
            "pb:webhooks": entries,
            "curies": [{ "name": "pb", "href": l.href("/doc/webhooks-{rel}"), "templated": true }]
        }
    })
}

/// `GET | POST /webhooks`
pub async fn collection(
    m: Method,
    State(state): State<AppState>,
    l: Links,
    body: Bytes,
) -> Result<Response> {
    scoped_collection(m, &state, &l, "Webhooks", "/webhooks", None, None, &body).await
}

async fn find_pacticipant(state: &AppState, role: &str, name: &str) -> Result<Pacticipant> {
    state
        .db
        .find_pacticipant(name)
        .await?
        .ok_or_else(|| Error::NotFound(format!("No {role} with name '{name}' found")))
}

/// `GET | POST /webhooks/provider/{p}/consumer/{c}`
pub async fn consumer_provider_collection(
    m: Method,
    State(state): State<AppState>,
    Path((provider, consumer)): Path<(String, String)>,
    l: Links,
    body: Bytes,
) -> Result<Response> {
    let p = find_pacticipant(&state, "provider", &provider).await?;
    let c = find_pacticipant(&state, "consumer", &consumer).await?;
    let path = format!(
        "/webhooks/provider/{}/consumer/{}",
        encode(&provider),
        encode(&consumer)
    );
    scoped_collection(m, &state, &l, "Webhooks", &path, Some(c), Some(p), &body).await
}

/// `GET | POST /webhooks/provider/{p}`
pub async fn provider_collection(
    m: Method,
    State(state): State<AppState>,
    Path(provider): Path<String>,
    l: Links,
    body: Bytes,
) -> Result<Response> {
    let p = find_pacticipant(&state, "provider", &provider).await?;
    let path = format!("/webhooks/provider/{}", encode(&provider));
    scoped_collection(m, &state, &l, "Webhooks", &path, None, Some(p), &body).await
}

/// `GET | POST /webhooks/consumer/{c}`
pub async fn consumer_collection(
    m: Method,
    State(state): State<AppState>,
    Path(consumer): Path<String>,
    l: Links,
    body: Bytes,
) -> Result<Response> {
    let c = find_pacticipant(&state, "consumer", &consumer).await?;
    let path = format!("/webhooks/consumer/{}", encode(&consumer));
    scoped_collection(m, &state, &l, "Webhooks", &path, Some(c), None, &body).await
}

/// `GET | POST /pacts/provider/{p}/consumer/{c}/webhooks`
pub async fn pact_collection(
    m: Method,
    State(state): State<AppState>,
    Path((provider, consumer)): Path<(String, String)>,
    l: Links,
    body: Bytes,
) -> Result<Response> {
    let p = find_pacticipant(&state, "provider", &provider).await?;
    let c = find_pacticipant(&state, "consumer", &consumer).await?;
    let path = format!(
        "/pacts/provider/{}/consumer/{}/webhooks",
        encode(&provider),
        encode(&consumer)
    );
    scoped_collection(
        m,
        &state,
        &l,
        "Pact webhooks",
        &path,
        Some(c),
        Some(p),
        &body,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn scoped_collection(
    m: Method,
    state: &AppState,
    l: &Links,
    title: &str,
    path: &str,
    consumer: Option<Pacticipant>,
    provider: Option<Pacticipant>,
    body: &Bytes,
) -> Result<Response> {
    match m {
        Method::GET => {
            let webhooks = if consumer.is_none() && provider.is_none() && path == "/webhooks" {
                state.db.list_webhooks().await?
            } else {
                state
                    .db
                    .webhooks_with_scope(
                        consumer.as_ref().map(|c| c.id),
                        provider.as_ref().map(|p| p.id),
                    )
                    .await?
            };
            Ok(Hal(collection_document(l, title, path, &webhooks)).into_response())
        }
        Method::POST => {
            let mut json = parse_body(body)?;
            // Verified: the scoped collections take the scope from the path.
            if let Some(c) = &consumer {
                json["consumer"] = json!({ "name": c.name });
            }
            if let Some(p) = &provider {
                json["provider"] = json!({ "name": p.name });
            }
            let req = WebhookRequest::parse(&json, None, &state.config)?;
            let w = service::create(&state.db, None, &req).await?;
            Ok((StatusCode::CREATED, Hal(webhook_document(l, &w))).into_response())
        }
        Method::OPTIONS => Ok(method::options(COLLECTION_ALLOWED)),
        _ => Ok(method::method_not_allowed(COLLECTION_ALLOWED)),
    }
}

/// `GET | PUT | DELETE /webhooks/{uuid}`
pub async fn webhook(
    m: Method,
    State(state): State<AppState>,
    Path(uuid): Path<String>,
    l: Links,
    body: Bytes,
) -> Result<Response> {
    match m {
        Method::GET => {
            let w = service::find(&state.db, &uuid).await?;
            Ok(Hal(webhook_document(&l, &w)).into_response())
        }
        Method::PUT => {
            let json = parse_body(&body)?;
            let req = WebhookRequest::parse(&json, Some(&uuid), &state.config)?;
            match state.db.find_webhook_by_uuid(&uuid).await? {
                Some(existing) => {
                    let w = service::update(&state.db, &existing, &req).await?;
                    Ok(Hal(webhook_document(&l, &w)).into_response())
                }
                None => {
                    let w = service::create(&state.db, Some(&uuid), &req).await?;
                    Ok((StatusCode::CREATED, Hal(webhook_document(&l, &w))).into_response())
                }
            }
        }
        Method::DELETE => {
            service::delete(&state.db, &uuid).await?;
            Ok(StatusCode::NO_CONTENT.into_response())
        }
        Method::OPTIONS => Ok(method::options(WEBHOOK_ALLOWED)),
        _ => Ok(method::method_not_allowed(WEBHOOK_ALLOWED)),
    }
}

/// `POST /webhooks/{uuid}/execute`
pub async fn execute(
    m: Method,
    State(state): State<AppState>,
    Path(uuid): Path<String>,
    _l: Links,
) -> Result<Response> {
    match m {
        Method::POST => {
            service::find(&state.db, &uuid).await?;
            Err(Error::NoRoute)
        }
        Method::OPTIONS => Ok(method::options(EXECUTE_ALLOWED)),
        _ => Ok(method::method_not_allowed(EXECUTE_ALLOWED)),
    }
}

/// `POST /webhooks/execute` (an unsaved webhook)
pub async fn execute_unsaved(
    m: Method,
    State(_state): State<AppState>,
    _l: Links,
    _body: Bytes,
) -> Result<Response> {
    match m {
        Method::POST => Err(Error::NoRoute),
        Method::OPTIONS => Ok(method::options(EXECUTE_ALLOWED)),
        _ => Ok(method::method_not_allowed(EXECUTE_ALLOWED)),
    }
}

// --- triggered webhooks -------------------------------------------------------------

fn attempts_remaining(state: &AppState, t: &TriggeredWebhook) -> usize {
    if t.status == "success" || t.status == "failure" {
        0
    } else {
        (state.config.webhook_retry_schedule.len() + 1).saturating_sub(t.executions.len())
    }
}

async fn triggered_webhook_document(
    state: &AppState,
    l: &Links,
    t: &TriggeredWebhook,
) -> Result<Value> {
    let webhook = match t.webhook_id {
        Some(id) => state.db.find_webhook_by_id(id).await?,
        None => None,
    };
    let name = webhook.as_ref().map(service::request_description);
    let logs = triggered_webhook_logs_href(l, &t.uuid);
    let mut doc = Map::new();
    if let Some(n) = &name {
        doc.insert("name".into(), json!(n));
    }
    doc.insert("status".into(), json!(t.status));
    doc.insert("attemptsMade".into(), json!(t.executions.len()));
    doc.insert(
        "attemptsRemaining".into(),
        json!(attempts_remaining(state, t)),
    );
    doc.insert("triggerType".into(), json!(t.trigger_type));
    doc.insert("eventName".into(), json!(t.event_name));
    doc.insert("triggeredAt".into(), json!(t.created_at));
    let mut links = Map::new();
    links.insert(
        "pb:logs".into(),
        json!({ "href": logs, "title": "Webhook execution logs", "name": name }),
    );
    links.insert(
        "logs".into(),
        json!({ "href": logs, "title": "DEPRECATED - Use pb:logs", "name": name }),
    );
    if let Some(w) = &webhook {
        links.insert(
            "pb:webhook".into(),
            json!({ "href": webhook_href(l, &w.uuid), "title": "Webhook", "name": name }),
        );
    }
    doc.insert("_links".into(), Value::Object(links));
    Ok(Value::Object(doc))
}

async fn triggered_webhooks_document(
    state: &AppState,
    l: &Links,
    title: &str,
    href: &str,
    rows: &[TriggeredWebhook],
) -> Result<Value> {
    let mut entries = vec![];
    for t in rows {
        entries.push(triggered_webhook_document(state, l, t).await?);
    }
    Ok(
        json!({ "_embedded": { "triggeredWebhooks": entries }, "_links": { "self": { "title": title, "href": href } } }),
    )
}

/// `GET /pacts/provider/{p}/consumer/{c}/version/{v}/triggered-webhooks`
pub async fn pact_triggered_webhooks(
    m: Method,
    State(state): State<AppState>,
    Path((provider, consumer, version)): Path<(String, String, String)>,
    l: Links,
) -> Result<Response> {
    match m {
        Method::GET => {
            let cv = state
                .db
                .find_version(&consumer, &version)
                .await?
                .ok_or(Error::NoRoute)?;
            let p = state
                .db
                .find_pacticipant(&provider)
                .await?
                .ok_or(Error::NoRoute)?;
            let pact = state
                .db
                .find_publication(cv.id, p.id)
                .await?
                .ok_or(Error::NoRoute)?;
            let rows = state
                .db
                .triggered_webhooks_for_pact_publication(pact.id)
                .await?;
            let href = format!(
                "{}/triggered-webhooks",
                pact_href(&l, &provider, &consumer, &version)
            );
            let title = format!(
                "Webhooks triggered by the publication of the pact between {consumer} ({version}) and {provider}"
            );
            Ok(
                Hal(triggered_webhooks_document(&state, &l, &title, &href, &rows).await?)
                    .into_response(),
            )
        }
        Method::OPTIONS => Ok(method::options(READ_ALLOWED)),
        _ => Ok(method::method_not_allowed(READ_ALLOWED)),
    }
}

/// `GET /pacts/provider/{p}/consumer/{c}/pact-version/{sha}/verification-results/{n}/triggered-webhooks`
pub async fn verification_triggered_webhooks(
    m: Method,
    State(state): State<AppState>,
    Path((provider, consumer, sha, number)): Path<(String, String, String, i64)>,
    l: Links,
) -> Result<Response> {
    match m {
        Method::GET => {
            state
                .db
                .find_pact_by_sha(&provider, &consumer, &sha)
                .await?
                .ok_or(Error::NoRoute)?;
            let v = state
                .db
                .find_verification_by_number(number)
                .await?
                .ok_or(Error::NoRoute)?;
            let rows = state.db.triggered_webhooks_for_verification(v.id).await?;
            let href = l.href(&format!("/pacts/provider/{}/consumer/{}/pact-version/{sha}/verification-results/{number}/triggered-webhooks", encode(&provider), encode(&consumer)));
            let title =
                format!("Webhooks triggered by the publication of verification result {number}");
            Ok(
                Hal(triggered_webhooks_document(&state, &l, &title, &href, &rows).await?)
                    .into_response(),
            )
        }
        Method::OPTIONS => Ok(method::options(READ_ALLOWED)),
        _ => Ok(method::method_not_allowed(READ_ALLOWED)),
    }
}

/// `GET /triggered-webhooks/{uuid}/logs`
pub async fn triggered_webhook_logs(
    m: Method,
    State(state): State<AppState>,
    Path(uuid): Path<String>,
    l: Links,
) -> Result<Response> {
    match m {
        Method::GET => {
            let t = state
                .db
                .find_triggered_webhook_by_uuid(&uuid)
                .await?
                .ok_or(Error::NoRoute)?;
            let executions: Vec<Value> = t
                .executions
                .iter()
                .map(|e| json!({ "success": e.success, "logs": e.logs, "createdAt": e.created_at }))
                .collect();
            let mut links = Map::new();
            links.insert("self".into(), json!({ "title": "Triggered webhook logs", "href": triggered_webhook_logs_href(&l, &t.uuid) }));
            if t.webhook_id.is_some() {
                links.insert(
                    "pb:webhook".into(),
                    json!({ "href": webhook_href(&l, &t.webhook_uuid), "title": "Webhook" }),
                );
            }
            Ok(Hal(json!({
                "eventName": t.event_name, "consumerName": t.consumer_name, "providerName": t.provider_name,
                "executions": executions,
                "_embedded": { "triggeredWebhook": { "uuid": t.uuid } },
                "_links": links
            })).into_response())
        }
        Method::OPTIONS => Ok(method::options(READ_ALLOWED)),
        _ => Ok(method::method_not_allowed(READ_ALLOWED)),
    }
}

/// `GET /pacts/provider/{p}/consumer/{c}/webhooks/status`
pub async fn pact_webhooks_status(
    m: Method,
    State(state): State<AppState>,
    Path((provider, consumer)): Path<(String, String)>,
    l: Links,
) -> Result<Response> {
    match m {
        Method::GET => {
            let p = find_pacticipant(&state, "provider", &provider).await?;
            let c = find_pacticipant(&state, "consumer", &consumer).await?;
            let rows = state.db.latest_triggered_webhooks(c.id, p.id).await?;
            let count = |status: &str| rows.iter().filter(|t| t.status == status).count();
            let mut summary = Map::new();
            summary.insert("successful".into(), json!(count("success")));
            summary.insert("failed".into(), json!(count("failure")));
            if count("retrying") > 0 {
                summary.insert("retrying".into(), json!(count("retrying")));
            }
            if count("not_run") > 0 {
                summary.insert("notRun".into(), json!(count("not_run")));
            }
            let mut entries = vec![];
            let mut error_logs = vec![];
            for t in &rows {
                entries.push(triggered_webhook_document(&state, &l, t).await?);
                if t.status == "failure" || t.status == "retrying" {
                    let name = match t.webhook_id {
                        Some(id) => state
                            .db
                            .find_webhook_by_id(id)
                            .await?
                            .map(|w| service::request_description(&w)),
                        None => None,
                    };
                    error_logs.push(json!({ "href": triggered_webhook_logs_href(&l, &t.uuid), "title": "Error logs", "name": name }));
                }
            }
            let mut links = Map::new();
            links.insert("self".into(), json!({ "href": l.href(&format!("/pacts/provider/{}/consumer/{}/webhooks/status", encode(&provider), encode(&consumer))), "title": "Webhooks status" }));
            links.insert("pb:error-logs".into(), json!(error_logs));
            links.insert("pb:pact-webhooks".into(), json!({ "title": format!("Webhooks for the pact between {consumer} and {provider}"), "href": l.href(&format!("/pacts/provider/{}/consumer/{}/webhooks", encode(&provider), encode(&consumer))) }));
            if let Some(first) = rows.first()
                && let Some(id) = first.pact_publication_id
                && let Some(pact) = state.db.find_publication_by_id(id).await?
            {
                links.insert("pb:pact-version".into(), json!({ "href": pact_href(&l, &pact.provider_name, &pact.consumer_name, &pact.consumer_version_number), "title": "Pact", "name": format!("Pact between {} ({}) and {}", pact.consumer_name, pact.consumer_version_number, pact.provider_name) }));
            }
            links.insert("pb:consumer".into(), json!({ "href": pacticipant_href(&l, &consumer), "title": "Consumer", "name": consumer }));
            links.insert("pb:provider".into(), json!({ "href": pacticipant_href(&l, &provider), "title": "Provider", "name": provider }));
            Ok(Hal(json!({ "summary": summary, "_embedded": { "triggeredWebhooks": entries }, "_links": links })).into_response())
        }
        Method::OPTIONS => Ok(method::options(READ_ALLOWED)),
        _ => Ok(method::method_not_allowed(READ_ALLOWED)),
    }
}
