//! `/environments`, deployed and released versions. Shapes follow the Phase 4 capture.
use axum::{
    body::Bytes,
    extract::{Path, RawQuery, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde_json::{Map, Value, json};

use crate::app::AppState;
use crate::db::environments::{DeployedVersion, Environment, ReleasedVersion};
use crate::error::{Error, Result};
use crate::http::hal::{HAL_CONTENT_TYPE, Hal, Links, encode};
use crate::http::method;
use crate::http::versions::{pacticipant_href, version_href};
use crate::service::environments::{self as service, EnvironmentRequest};

const ENVIRONMENTS_ALLOWED: &str = "GET, POST, OPTIONS";
const ENVIRONMENT_ALLOWED: &str = "GET, PUT, DELETE, OPTIONS";
const RECORD_ALLOWED: &str = "GET, POST, OPTIONS";
const RECORD_DOC_ALLOWED: &str = "GET, PATCH, OPTIONS";
const READ_ALLOWED: &str = "GET, OPTIONS";

fn parse_body(body: &Bytes) -> Result<Value> {
    if body.is_empty() {
        return Ok(json!({}));
    }
    serde_json::from_slice(body).map_err(|e| Error::BadRequest(format!("JSON::ParserError - {e}")))
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

pub fn environment_href(l: &Links, e: &Environment) -> String {
    l.href(&format!("/environments/{}", e.uuid))
}

pub fn display_name(e: &Environment) -> &str {
    e.display_name.as_deref().unwrap_or(&e.name)
}

/// The full environment document.
pub fn environment_document(l: &Links, e: &Environment) -> Value {
    let href = environment_href(l, e);
    let mut doc = Map::new();
    doc.insert("uuid".into(), json!(e.uuid));
    doc.insert("name".into(), json!(e.name));
    if let Some(d) = &e.display_name {
        doc.insert("displayName".into(), json!(d));
    }
    doc.insert("production".into(), json!(e.production));
    if let Some(c) = e
        .contacts
        .as_deref()
        .and_then(|c| serde_json::from_str::<Value>(c).ok())
    {
        doc.insert("contacts".into(), c);
    }
    if let Some(u) = &e.updated_at {
        doc.insert("updatedAt".into(), json!(u));
    }
    doc.insert("createdAt".into(), json!(e.created_at));
    let dn = display_name(e);
    doc.insert("_links".into(), json!({
        "self": { "title": "Environment", "name": e.name, "href": href },
        "pb:currently-deployed-deployed-versions": { "title": format!("Versions currently deployed to {dn} environment"), "href": format!("{href}/deployed-versions/currently-deployed") },
        "pb:currently-supported-released-versions": { "title": format!("Versions released and supported in {dn} environment"), "href": format!("{href}/released-versions/currently-supported") },
        "pb:environments": { "title": "Environments", "href": l.href("/environments") }
    }));
    Value::Object(doc)
}

/// The short environment stub embedded in matrix rows (verified: no contacts, only self).
pub fn environment_stub(l: &Links, e: &Environment) -> Value {
    let mut doc = Map::new();
    doc.insert("uuid".into(), json!(e.uuid));
    doc.insert("name".into(), json!(e.name));
    if let Some(d) = &e.display_name {
        doc.insert("displayName".into(), json!(d));
    }
    doc.insert("production".into(), json!(e.production));
    if let Some(u) = &e.updated_at {
        doc.insert("updatedAt".into(), json!(u));
    }
    doc.insert("createdAt".into(), json!(e.created_at));
    doc.insert("_links".into(), json!({ "self": { "title": "Environment", "name": e.name, "href": environment_href(l, e) } }));
    Value::Object(doc)
}

/// `GET | POST /environments`
pub async fn environments(
    m: Method,
    State(state): State<AppState>,
    l: Links,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response> {
    match m {
        Method::GET => {
            let all = state.db.list_environments().await?;
            let docs: Vec<Value> = all.iter().map(|e| environment_document(&l, e)).collect();
            let links: Vec<Value> = all.iter().map(|e| json!({ "title": "Environment", "name": e.name, "href": environment_href(&l, e) })).collect();
            Ok(Hal(json!({
                "_embedded": { "environments": docs },
                "_links": { "self": { "title": "Environments", "href": l.href("/environments") }, "pb:environments": links }
            }))
            .into_response())
        }
        Method::POST => {
            if !has_json_content_type(&headers) {
                return Ok(unsupported_media_type());
            }
            let req = EnvironmentRequest::parse(&parse_body(&body)?)?;
            let e = service::create(&state.db, &req).await?;
            Ok((StatusCode::CREATED, Hal(environment_document(&l, &e))).into_response())
        }
        Method::OPTIONS => Ok(method::options(ENVIRONMENTS_ALLOWED)),
        _ => Ok(method::method_not_allowed(ENVIRONMENTS_ALLOWED)),
    }
}

/// `GET | PUT | DELETE /environments/{uuid}`
pub async fn environment(
    m: Method,
    State(state): State<AppState>,
    Path(uuid): Path<String>,
    l: Links,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response> {
    match m {
        Method::GET => Ok(Hal(environment_document(
            &l,
            &service::find(&state.db, &uuid).await?,
        ))
        .into_response()),
        Method::PUT => {
            if !has_json_content_type(&headers) {
                return Ok(unsupported_media_type());
            }
            let req = EnvironmentRequest::parse(&parse_body(&body)?)?;
            let e = service::update(&state.db, &uuid, &req).await?;
            Ok(Hal(environment_document(&l, &e)).into_response())
        }
        Method::DELETE => {
            service::delete(&state.db, &uuid).await?;
            Ok(StatusCode::NO_CONTENT.into_response())
        }
        Method::OPTIONS => Ok(method::options(ENVIRONMENT_ALLOWED)),
        _ => Ok(method::method_not_allowed(ENVIRONMENT_ALLOWED)),
    }
}

// --- deployed versions ----------------------------------------------------------------

/// The deployed-version document; `environment` embeds the full environment document.
pub fn deployed_version_document(l: &Links, d: &DeployedVersion, e: Option<&Environment>) -> Value {
    let mut doc = Map::new();
    doc.insert("uuid".into(), json!(d.uuid));
    doc.insert("currentlyDeployed".into(), json!(d.currently_deployed));
    if let Some(i) = &d.application_instance {
        doc.insert("target".into(), json!(i));
        doc.insert("applicationInstance".into(), json!(i));
    }
    if let Some(u) = &d.undeployed_at {
        doc.insert("undeployedAt".into(), json!(u));
    }
    doc.insert("createdAt".into(), json!(d.created_at));
    let mut embedded = Map::new();
    embedded.insert("pacticipant".into(), json!({ "name": d.pacticipant_name, "_links": { "self": { "href": pacticipant_href(l, &d.pacticipant_name) } } }));
    embedded.insert("version".into(), json!({ "number": d.version_number, "_links": { "self": { "title": "Version", "name": d.version_number, "href": version_href(l, &d.pacticipant_name, &d.version_number) } } }));
    if let Some(e) = e {
        embedded.insert("environment".into(), environment_document(l, e));
    }
    doc.insert("_embedded".into(), Value::Object(embedded));
    doc.insert(
        "_links".into(),
        json!({ "self": { "href": l.href(&format!("/deployed-versions/{}", d.uuid)) } }),
    );
    Value::Object(doc)
}

pub fn released_version_document(l: &Links, r: &ReleasedVersion, e: Option<&Environment>) -> Value {
    let mut doc = Map::new();
    doc.insert("uuid".into(), json!(r.uuid));
    doc.insert("currentlySupported".into(), json!(r.currently_supported));
    doc.insert("createdAt".into(), json!(r.created_at));
    if let Some(u) = &r.support_ended_at {
        doc.insert("supportEndedAt".into(), json!(u));
    }
    let mut embedded = Map::new();
    embedded.insert("pacticipant".into(), json!({ "name": r.pacticipant_name, "_links": { "self": { "href": pacticipant_href(l, &r.pacticipant_name) } } }));
    embedded.insert("version".into(), json!({ "number": r.version_number, "_links": { "self": { "title": "Version", "name": r.version_number, "href": version_href(l, &r.pacticipant_name, &r.version_number) } } }));
    if let Some(e) = e {
        embedded.insert("environment".into(), environment_document(l, e));
    }
    doc.insert("_embedded".into(), Value::Object(embedded));
    doc.insert(
        "_links".into(),
        json!({ "self": { "href": l.href(&format!("/released-versions/{}", r.uuid)) } }),
    );
    Value::Object(doc)
}

/// `POST /pacticipants/{p}/versions/{v}/deployed-versions/environment/{uuid}`
pub async fn record_deployment(
    m: Method,
    State(state): State<AppState>,
    Path((p, v, uuid)): Path<(String, String, String)>,
    l: Links,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response> {
    match m {
        Method::POST => {
            if !has_json_content_type(&headers) {
                return Ok(unsupported_media_type());
            }
            let json = parse_body(&body)?;
            let instance = json
                .get("applicationInstance")
                .or_else(|| json.get("target"))
                .and_then(|x| x.as_str())
                .filter(|s| !s.is_empty())
                .map(str::to_owned);
            let (d, e, _) =
                service::record_deployment(&state.db, &p, &v, &uuid, instance.as_deref()).await?;
            // The Ruby broker answers 201 even when the same deployment already exists (verified).
            Ok((
                StatusCode::CREATED,
                Hal(deployed_version_document(&l, &d, Some(&e))),
            )
                .into_response())
        }
        Method::OPTIONS => Ok(method::options(RECORD_ALLOWED)),
        _ => Ok(method::method_not_allowed(RECORD_ALLOWED)),
    }
}

/// `POST /pacticipants/{p}/versions/{v}/released-versions/environment/{uuid}`
pub async fn record_release(
    m: Method,
    State(state): State<AppState>,
    Path((p, v, uuid)): Path<(String, String, String)>,
    l: Links,
    headers: HeaderMap,
) -> Result<Response> {
    match m {
        Method::POST => {
            if !has_json_content_type(&headers) {
                return Ok(unsupported_media_type());
            }
            let (r, e, created) = service::record_release(&state.db, &p, &v, &uuid).await?;
            let status = if created {
                StatusCode::CREATED
            } else {
                StatusCode::OK
            };
            Ok((status, Hal(released_version_document(&l, &r, Some(&e)))).into_response())
        }
        Method::OPTIONS => Ok(method::options(RECORD_ALLOWED)),
        _ => Ok(method::method_not_allowed(RECORD_ALLOWED)),
    }
}

/// `GET | PATCH /deployed-versions/{uuid}`
pub async fn deployed_version(
    m: Method,
    State(state): State<AppState>,
    Path(uuid): Path<String>,
    l: Links,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response> {
    match m {
        Method::GET => {
            let d = state
                .db
                .find_deployed_version(&uuid)
                .await?
                .ok_or(Error::NoRoute)?;
            let e = state.db.find_environment_by_id(d.environment_id).await?;
            Ok(Hal(deployed_version_document(&l, &d, e.as_ref())).into_response())
        }
        Method::PATCH => {
            if !has_json_content_type(&headers) {
                return Ok(unsupported_media_type());
            }
            let d = service::patch_deployed_version(&state.db, &uuid, &parse_body(&body)?).await?;
            let e = state.db.find_environment_by_id(d.environment_id).await?;
            Ok(Hal(deployed_version_document(&l, &d, e.as_ref())).into_response())
        }
        Method::OPTIONS => Ok(method::options(RECORD_DOC_ALLOWED)),
        _ => Ok(method::method_not_allowed(RECORD_DOC_ALLOWED)),
    }
}

/// `GET | PATCH /released-versions/{uuid}`
pub async fn released_version(
    m: Method,
    State(state): State<AppState>,
    Path(uuid): Path<String>,
    l: Links,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response> {
    match m {
        Method::GET => {
            let r = state
                .db
                .find_released_version(&uuid)
                .await?
                .ok_or(Error::NoRoute)?;
            let e = state.db.find_environment_by_id(r.environment_id).await?;
            Ok(Hal(released_version_document(&l, &r, e.as_ref())).into_response())
        }
        Method::PATCH => {
            if !has_json_content_type(&headers) {
                return Ok(unsupported_media_type());
            }
            let r = service::patch_released_version(&state.db, &uuid, &parse_body(&body)?).await?;
            let e = state.db.find_environment_by_id(r.environment_id).await?;
            Ok(Hal(released_version_document(&l, &r, e.as_ref())).into_response())
        }
        Method::OPTIONS => Ok(method::options(RECORD_DOC_ALLOWED)),
        _ => Ok(method::method_not_allowed(RECORD_DOC_ALLOWED)),
    }
}

fn query_param(query: Option<&str>, key: &str) -> Option<String> {
    query?.split('&').find_map(|pair| {
        let (k, v) = pair.split_once('=')?;
        (k == key).then(|| v.replace("%20", " ").replace('+', " "))
    })
}

/// `GET /environments/{uuid}/deployed-versions/currently-deployed[?pacticipant=]`
pub async fn currently_deployed(
    m: Method,
    State(state): State<AppState>,
    Path(uuid): Path<String>,
    RawQuery(query): RawQuery,
    l: Links,
) -> Result<Response> {
    match m {
        Method::GET => {
            let e = service::find(&state.db, &uuid).await?;
            let pacticipant = query_param(query.as_deref(), "pacticipant");
            let rows = state
                .db
                .currently_deployed(e.id, pacticipant.as_deref())
                .await?;
            let docs: Vec<Value> = rows
                .iter()
                .map(|d| deployed_version_document(&l, d, None))
                .collect();
            let mut href = format!(
                "{}/deployed-versions/currently-deployed",
                environment_href(&l, &e)
            );
            if let Some(p) = &pacticipant {
                href.push_str(&format!("?pacticipant={}", encode(p)));
            }
            Ok(Hal(json!({
                "_embedded": { "deployedVersions": docs },
                "_links": { "self": { "href": href, "title": format!("Currently deployed versions for {}", display_name(&e)) } }
            }))
            .into_response())
        }
        Method::OPTIONS => Ok(method::options(READ_ALLOWED)),
        _ => Ok(method::method_not_allowed(READ_ALLOWED)),
    }
}

/// `GET /environments/{uuid}/released-versions/currently-supported[?pacticipant=&version=]`
pub async fn currently_supported(
    m: Method,
    State(state): State<AppState>,
    Path(uuid): Path<String>,
    RawQuery(query): RawQuery,
    l: Links,
) -> Result<Response> {
    match m {
        Method::GET => {
            let e = service::find(&state.db, &uuid).await?;
            let pacticipant = query_param(query.as_deref(), "pacticipant");
            let version = query_param(query.as_deref(), "version");
            let rows = state
                .db
                .currently_supported(e.id, pacticipant.as_deref(), version.as_deref())
                .await?;
            let docs: Vec<Value> = rows
                .iter()
                .map(|r| released_version_document(&l, r, None))
                .collect();
            let mut href = format!(
                "{}/released-versions/currently-supported",
                environment_href(&l, &e)
            );
            let mut params = vec![];
            if let Some(p) = &pacticipant {
                params.push(format!("pacticipant={}", encode(p)));
            }
            if let Some(v) = &version {
                params.push(format!("version={}", encode(v)));
            }
            if !params.is_empty() {
                href.push('?');
                href.push_str(&params.join("&"));
            }
            Ok(Hal(json!({
                "_embedded": { "releasedVersions": docs },
                "_links": { "self": { "href": href, "title": format!("Currently supported versions in {}", display_name(&e)) } }
            }))
            .into_response())
        }
        Method::OPTIONS => Ok(method::options(READ_ALLOWED)),
        _ => Ok(method::method_not_allowed(READ_ALLOWED)),
    }
}
