//! Pact documents and lists under `/pacts/...`. Shapes follow the Phase 2 capture.
use axum::{
    body::Bytes,
    extract::{Path, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode, header},
    response::{IntoResponse, Response},
};
use base64::Engine;
use serde_json::{Map, Value, json};

use crate::app::AppState;
use crate::db::pacts::PactPublication;
use crate::error::{Error, Result};
use crate::http::hal::{HAL_CONTENT_TYPE, Hal, JSON_CONTENT_TYPE, Links, encode};
use crate::http::method;
use crate::http::versions::{pacticipant_href, version_href};
use crate::service::content;
use crate::service::publish;

const PACT_ALLOWED: &str = "GET, PUT, DELETE, PATCH, OPTIONS";
const READ_ALLOWED: &str = "GET, OPTIONS";

pub fn pact_href(l: &Links, provider: &str, consumer: &str, version: &str) -> String {
    l.href(&format!(
        "/pacts/provider/{}/consumer/{}/version/{}",
        encode(provider),
        encode(consumer),
        encode(version)
    ))
}

fn integration_href(l: &Links, provider: &str, consumer: &str) -> String {
    l.href(&format!(
        "/pacts/provider/{}/consumer/{}",
        encode(provider),
        encode(consumer)
    ))
}

/// Opaque metadata the Ruby broker threads through the verification-results href of
/// pacts fetched by "latest" routes, so the result can be tied to the consumer version
/// it was fetched for: url-safe base64 without padding of `cv=<consumer version id>`.
pub fn latest_metadata(p: &PactPublication) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(format!("cv={}", p.consumer_version_id))
}

/// The pact content with `_id`s, `createdAt` and the Ruby broker's links. `metadata`
/// is inserted into the `pb:publish-verification-results` href when given.
pub fn pact_document(l: &Links, p: &PactPublication, metadata: Option<&str>) -> Value {
    let content: Value = serde_json::from_str(&p.content).expect("stored json");
    let mut doc = match content::with_ids(content) {
        Value::Object(m) => m,
        other => {
            let mut m = Map::new();
            m.insert("content".into(), other);
            m
        }
    };
    let (c, pr, v) = (
        &p.consumer_name,
        &p.provider_name,
        &p.consumer_version_number,
    );
    let base = integration_href(l, pr, c);
    let this = pact_href(l, pr, c, v);
    let pact_version = format!("{base}/pact-version/{}", p.sha);
    let results_base = match metadata {
        Some(m) => format!("{pact_version}/metadata/{m}"),
        None => pact_version.clone(),
    };
    doc.insert("createdAt".into(), json!(p.created_at));
    doc.insert("_links".into(), json!({
        "self": { "title": "Pact", "name": format!("Pact between {c} ({v}) and {pr}"), "href": this },
        "pb:consumer": { "title": "Consumer", "name": c, "href": pacticipant_href(l, c) },
        "pb:consumer-version": { "title": "Consumer version", "name": v, "href": version_href(l, c, v) },
        "pb:provider": { "title": "Provider", "name": pr, "href": pacticipant_href(l, pr) },
        "pb:pact-version": { "title": "Pact content version permalink", "name": p.sha, "href": pact_version },
        "pb:latest-pact-version": { "title": "Latest version of this pact", "href": format!("{base}/latest") },
        "pb:all-pact-versions": { "title": "All versions of this pact", "href": format!("{base}/versions") },
        "pb:latest-untagged-pact-version": { "title": "Latest untagged version of this pact", "href": format!("{base}/latest-untagged") },
        "pb:latest-tagged-pact-version": { "title": "Latest tagged version of this pact", "href": format!("{base}/latest/{{tag}}"), "templated": true },
        "pb:previous-distinct": { "title": "Previous distinct version of this pact", "href": format!("{this}/previous-distinct") },
        "pb:diff-previous-distinct": { "title": "Diff with previous distinct version of this pact", "href": format!("{this}/diff/previous-distinct") },
        "pb:diff": { "title": "Diff with another specified version of this pact", "href": format!("{pact_version}/diff/pact-version/{{pactVersion}}"), "templated": true },
        "pb:pact-webhooks": { "title": format!("Webhooks for the pact between {c} and {pr}"), "href": l.href(&format!("/webhooks/provider/{}/consumer/{}", encode(pr), encode(c))) },
        "pb:consumer-webhooks": { "title": format!("Webhooks for all pacts with provider {pr}"), "href": l.href(&format!("/webhooks/consumer/{}", encode(pr))) },
        "pb:tag-prod-version": { "title": "PUT to this resource to tag this consumer version as 'production'", "href": format!("{}/tags/prod", version_href(l, c, v)) },
        "pb:tag-version": { "title": "PUT to this resource to tag this consumer version", "href": format!("{}/tags/{{tag}}", version_href(l, c, v)) },
        "pb:publish-verification-results": { "title": "Publish verification results", "href": format!("{results_base}/verification-results") },
        "pb:latest-verification-results": { "href": format!("{pact_version}/verification-results/latest") },
        "pb:triggered-webhooks": { "title": "Webhooks triggered by the publication of this pact", "href": format!("{this}/triggered-webhooks") },
        "pb:matrix-for-consumer-version": { "title": "View matrix rows for the consumer version to which this pact belongs",
            "href": l.href(&format!("/matrix?q%5B%5D%5Bpacticipant%5D={}&q%5B%5D%5Bversion%5D={}&latestby=cvpv", encode(c), encode(v))) },
        "curies": [{ "name": "pb", "href": l.href("/doc/{rel}?context=pact"), "templated": true }]
    }));
    Value::Object(doc)
}

/// Pact documents are HAL unless the client asks for exactly `application/json`.
fn render_pact(headers: &HeaderMap, doc: Value) -> Response {
    let wants_plain = headers
        .get(header::ACCEPT)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|a| a.trim() == "application/json");
    let mut res = axum::Json(doc).into_response();
    let ct = if wants_plain {
        JSON_CONTENT_TYPE
    } else {
        HAL_CONTENT_TYPE
    };
    res.headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static(ct));
    res
}

fn has_json_content_type(headers: &HeaderMap) -> bool {
    headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|ct| ct.contains("json"))
}

fn unsupported_media_type() -> Response {
    (
        StatusCode::UNSUPPORTED_MEDIA_TYPE,
        [(
            header::CONTENT_TYPE,
            HeaderValue::from_static(HAL_CONTENT_TYPE),
        )],
    )
        .into_response()
}

/// `GET | PUT | DELETE /pacts/provider/{p}/consumer/{c}/version/{v}`
pub async fn pact_by_version(
    m: Method,
    State(state): State<AppState>,
    Path((provider, consumer, version)): Path<(String, String, String)>,
    l: Links,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response> {
    match m {
        Method::GET => {
            let p = state
                .db
                .find_pact(&provider, &consumer, &version)
                .await?
                .ok_or(Error::NoRoute)?;
            Ok(render_pact(&headers, pact_document(&l, &p, None)))
        }
        Method::PUT => {
            if !has_json_content_type(&headers) {
                return Ok(unsupported_media_type());
            }
            let json: Value = serde_json::from_slice(&body)
                .map_err(|e| Error::BadRequest(format!("JSON::ParserError - {e}")))?;
            let (p, created) =
                publish::put_pact(&state.db, &provider, &consumer, &version, json).await?;
            let mut events = vec!["contract_published"];
            if !crate::service::webhook_trigger::required_verifications(&state.db, &p)
                .await?
                .is_empty()
            {
                events.push("contract_requiring_verification_published");
            }
            if created {
                events.push("contract_content_changed");
            }
            crate::service::webhook_trigger::trigger_publish_events(
                &state.db,
                &state.config,
                &l.href(""),
                &p,
                &events,
                None,
            )
            .await?;
            let mut res = render_pact(&headers, pact_document(&l, &p, None));
            if created {
                *res.status_mut() = StatusCode::CREATED;
            }
            Ok(res)
        }
        Method::DELETE => {
            let p = state
                .db
                .find_pact(&provider, &consumer, &version)
                .await?
                .ok_or(Error::NoRoute)?;
            state.db.delete_publication(p.id).await?;
            Ok(Hal(json!({
                "_links": {
                    "index": { "href": l.href("") },
                    "pb:latest-pact-version": { "href": format!("{}/latest", integration_href(&l, &provider, &consumer)), "title": "Latest pact" }
                }
            }))
            .into_response())
        }
        Method::PATCH => Ok(unsupported_media_type()),
        Method::OPTIONS => Ok(method::options(PACT_ALLOWED)),
        _ => Ok(method::method_not_allowed(PACT_ALLOWED)),
    }
}

/// `GET /pacts/provider/{p}/consumer/{c}/latest`
pub async fn latest_pact(
    m: Method,
    State(state): State<AppState>,
    Path((provider, consumer)): Path<(String, String)>,
    l: Links,
    headers: HeaderMap,
) -> Result<Response> {
    match m {
        Method::GET => {
            let p = state
                .db
                .latest_pact(&provider, &consumer)
                .await?
                .ok_or(Error::NoRoute)?;
            Ok(render_pact(
                &headers,
                pact_document(&l, &p, Some(&latest_metadata(&p))),
            ))
        }
        Method::OPTIONS => Ok(method::options(READ_ALLOWED)),
        _ => Ok(method::method_not_allowed(READ_ALLOWED)),
    }
}

/// `GET /pacts/provider/{p}/consumer/{c}/latest/{tag}`
pub async fn latest_pact_with_tag(
    m: Method,
    State(state): State<AppState>,
    Path((provider, consumer, tag)): Path<(String, String, String)>,
    l: Links,
    headers: HeaderMap,
) -> Result<Response> {
    match m {
        Method::GET => {
            let p = state
                .db
                .latest_pact_with_tag(&provider, &consumer, &tag)
                .await?
                .ok_or(Error::NoRoute)?;
            Ok(render_pact(
                &headers,
                pact_document(&l, &p, Some(&latest_metadata(&p))),
            ))
        }
        Method::OPTIONS => Ok(method::options(READ_ALLOWED)),
        _ => Ok(method::method_not_allowed(READ_ALLOWED)),
    }
}

/// `GET /pacts/provider/{p}/consumer/{c}/latest-untagged`
pub async fn latest_pact_untagged(
    m: Method,
    State(state): State<AppState>,
    Path((provider, consumer)): Path<(String, String)>,
    l: Links,
    headers: HeaderMap,
) -> Result<Response> {
    match m {
        Method::GET => {
            let p = state
                .db
                .latest_pact_untagged(&provider, &consumer)
                .await?
                .ok_or(Error::NoRoute)?;
            Ok(render_pact(
                &headers,
                pact_document(&l, &p, Some(&latest_metadata(&p))),
            ))
        }
        Method::OPTIONS => Ok(method::options(READ_ALLOWED)),
        _ => Ok(method::method_not_allowed(READ_ALLOWED)),
    }
}

/// `GET /pacts/provider/{p}/consumer/{c}/pact-version/{sha}`
pub async fn pact_by_sha(
    m: Method,
    State(state): State<AppState>,
    Path((provider, consumer, sha)): Path<(String, String, String)>,
    l: Links,
    headers: HeaderMap,
) -> Result<Response> {
    match m {
        Method::GET => {
            let p = state
                .db
                .find_pact_by_sha(&provider, &consumer, &sha)
                .await?
                .ok_or(Error::NoRoute)?;
            Ok(render_pact(&headers, pact_document(&l, &p, None)))
        }
        Method::OPTIONS => Ok(method::options(READ_ALLOWED)),
        _ => Ok(method::method_not_allowed(READ_ALLOWED)),
    }
}
