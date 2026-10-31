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
use crate::error::{Error, Result};
use crate::http::hal::{Hal, Links, encode};
use crate::http::method;
use crate::http::pacts::pact_href;
use crate::http::versions::{pacticipant_href, version_href, version_stub};
use crate::service::publish::{self as service, PublishOutcome, PublishRequest};

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
            match service::publish_contracts(&state.db, &req).await {
                Ok(outcome) => Ok(Hal(document(&l, &req, &outcome)).into_response()),
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

fn document(l: &Links, req: &PublishRequest, o: &PublishOutcome) -> Value {
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
    for c in &o.contracts {
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
        notices.push((
            "debug",
            format!("  Events detected: {}{reasons}", c.events.join(", ")),
        ));
        notices.push(("prompt", "  Next steps:".into()));
        notices.push(("prompt", format!("    * Add Pact verification tests to the {provider} build. See https://docs.pact.io/go/provider_verification")));
        notices.push(("prompt", format!("    * Configure separate {provider} pact verification build and webhook to trigger it when the pact content changes. See https://docs.pact.io/go/webhooks")));
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
    json!({
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
    })
}
