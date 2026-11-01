//! `POST /contracts/publish`, the `pb:publish-contracts` relation the CLI and pact clients
//! use. Response shape and notice wording follow the Phase 2 capture.
use axum::{
    body::Bytes,
    extract::State,
    http::{Method, StatusCode},
    response::{IntoResponse, Response},
};
use serde_json::{Value, json};

use crate::app::AppState;
use crate::db::webhooks::TriggeredWebhook;
use crate::error::{Error, Result};
use crate::http::hal::{Hal, Links, encode};
use crate::http::method;
use crate::http::pacts::pact_href;
use crate::http::versions::{pacticipant_href, version_href, version_stub};
use crate::http::webhooks::triggered_webhook_logs_href;
use crate::service::publish::{self as service, PublishOutcome, PublishRequest};
use crate::service::webhook_trigger as trigger;
use crate::service::webhooks;

const ALLOWED: &str = "POST, OPTIONS";

pub async fn publish(
    m: Method,
    State(state): State<AppState>,
    l: Links,
    body: Bytes,
) -> Result<Response> {
    match m {
        Method::POST => {
            let json: Value = serde_json::from_slice(&body)
                .map_err(|e| Error::BadRequest(format!("JSON::ParserError - {e}")))?;
            let req = PublishRequest::parse(&json)?;
            // The batch, its metadata and its webhook triggers land together or not at all.
            let tx = state.db.begin().await?;
            match service::publish_contracts(&tx, &req).await {
                Ok(outcome) => {
                    let mut triggered = vec![];
                    for c in &outcome.contracts {
                        let fired = trigger::trigger_publish_events(
                            &tx,
                            &state.config,
                            &l.href(""),
                            &c.publication,
                            &c.events,
                            req.build_url.as_deref(),
                        )
                        .await?;
                        let configured = trigger::any_webhook_for(&tx, &c.publication).await?;
                        let verified = tx
                            .any_verifications_for_integration(
                                c.publication.consumer_id,
                                c.publication.provider_id,
                            )
                            .await?;
                        triggered.push((fired, configured, verified));
                    }
                    tx.commit().await?;
                    Ok(
                        Hal(document(&state, &l, &req, &outcome, &triggered).await?)
                            .into_response(),
                    )
                }
                Err(Error::Conflict(msg)) => Ok((
                    StatusCode::CONFLICT,
                    Hal(json!({
                        "notices": [{ "type": "error", "text": msg }],
                        "errors": { "contracts": [msg] }
                    })),
                )
                    .into_response()),
                Err(e) => Err(e),
            }
        }
        Method::OPTIONS => Ok(method::options(ALLOWED)),
        _ => Ok(method::method_not_allowed(ALLOWED)),
    }
}

/// `triggered`: per contract, the webhooks fired and whether any webhook is configured
/// for the pact when none fired (verified wording in the Phase 5 capture).
async fn document(
    state: &AppState,
    l: &Links,
    req: &PublishRequest,
    o: &PublishOutcome,
    triggered: &[(Vec<TriggeredWebhook>, bool, bool)],
) -> Result<Value> {
    let p = &o.pacticipant.name;
    let v = &o.version.number;
    let mut notices: Vec<(&str, String)> = vec![];
    let mut with = vec![];
    if let Some(b) = &req.branch {
        with.push(format!("branch {b}"));
    }
    if !req.tags.is_empty() {
        with.push(format!("tags {}", req.tags.join(", ")));
    }
    let with = if with.is_empty() {
        String::new()
    } else {
        format!(" with {}", with.join(" and "))
    };
    let verb = if o.version_created {
        "Created"
    } else {
        "Updated"
    };
    notices.push(("debug", format!("{verb} {p} version {v}{with}")));
    if req.branch.is_none() {
        notices.push(("prompt", "  Next steps:\n    Configure the version branch to be the value of your repository branch.".into()));
    }
    for (i, c) in o.contracts.iter().enumerate() {
        let (fired, configured, verified) = &triggered[i];
        let provider = &c.publication.provider_name;
        let consumer = &c.publication.consumer_name;
        if c.created {
            notices.push(("success", format!("Pact successfully published for {consumer} version {v} and provider {provider}.")));
        } else {
            notices.push(("success", format!("Pact successfully republished for {consumer} version {v} and provider {provider} with no content changes.")));
        }
        notices.push((
            "debug",
            format!(
                "  View the published pact at {}",
                pact_href(l, provider, consumer, v)
            ),
        ));
        let reasons = if c.reasons.is_empty() {
            String::new()
        } else {
            format!(" ({})", c.reasons.join(", "))
        };
        // Verified: when the content changed only contract_content_changed carries the
        // explanation; when it did not, every event carries it.
        let changed = c.events.contains(&"contract_content_changed");
        let described: Vec<String> = c
            .events
            .iter()
            .map(|e| {
                if !changed || *e == "contract_content_changed" {
                    format!("{e}{reasons}")
                } else {
                    e.to_string()
                }
            })
            .collect();
        notices.push((
            "debug",
            format!("  Events detected: {}", described.join(", ")),
        ));
        if !fired.is_empty() {
            for t in fired {
                let w = match t.webhook_id {
                    Some(id) => state.db.find_webhook_by_id(id).await?,
                    None => None,
                };
                let name = w
                    .as_ref()
                    .map(webhooks::notice_name)
                    .unwrap_or_else(|| t.webhook_uuid.clone());
                notices.push((
                    "debug",
                    format!(
                        "  Webhook {name} triggered for event {}.\n    View logs at {}",
                        t.event_name,
                        triggered_webhook_logs_href(l, &t.uuid)
                    ),
                ));
            }
        } else if *configured {
            notices.push((
                "debug",
                "  No enabled webhooks found for the detected events".into(),
            ));
        }
        // Verified (`next_steps_notices`): the verification step until the integration has
        // a verification, the webhook step until a webhook covers the pact.
        let mut next_steps = vec![];
        if !*verified {
            next_steps.push(format!("    * Add Pact verification tests to the {provider} build. See https://docs.pact.io/go/provider_verification"));
        }
        if !*configured {
            next_steps.push(format!("    * Configure separate {provider} pact verification build and webhook to trigger it when the pact content changes. See https://docs.pact.io/go/webhooks"));
        }
        if !next_steps.is_empty() {
            notices.push(("prompt", "  Next steps:".into()));
            for step in next_steps {
                notices.push(("prompt", step));
            }
        }
    }
    let logs: Vec<Value> = notices
        .iter()
        .map(|(t, text)| json!({ "level": t, "message": text, "deprecationWarning": "Replaced by notices" }))
        .collect();
    let notices: Vec<Value> = notices
        .iter()
        .map(|(t, text)| json!({ "type": t, "text": text }))
        .collect();
    let tags: Vec<Value> = req
        .tags
        .iter()
        .map(|t| json!({ "title": "Tag", "name": t, "href": format!("{}/tags/{}", version_href(l, p, v), encode(t)) }))
        .collect();
    let contracts: Vec<Value> = o
        .contracts
        .iter()
        .map(|c| {
            json!({
                "title": "Pact",
                "name": format!("Pact between {} ({v}) and {}", c.publication.consumer_name, c.publication.provider_name),
                "href": pact_href(l, &c.publication.provider_name, &c.publication.consumer_name, v)
            })
        })
        .collect();
    Ok(json!({
        "logs": logs,
        "notices": notices,
        "_embedded": {
            "pacticipant": { "name": p, "_links": { "self": { "href": pacticipant_href(l, p) } } },
            "version": version_stub(l, &o.version)
        },
        "_links": {
            "pb:pacticipant": { "title": "Pacticipant", "name": p, "href": pacticipant_href(l, p) },
            "pb:pacticipant-version": { "title": "Pacticipant version", "name": v, "href": version_href(l, p, v) },
            "pb:pacticipant-version-tags": tags,
            "pb:contracts": contracts
        }
    }))
}
