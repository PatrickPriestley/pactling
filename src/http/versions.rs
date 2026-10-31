//! `/pacticipants/{p}/versions...`, `/branches...`, `/latest-version...`. Documents match
//! the phase 2 fixtures; see tests/versions.rs.
use axum::{
    body::Bytes,
    extract::{Path, RawQuery, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde_json::{Map, Value, json};

use crate::app::AppState;
use crate::db::versions::{Branch, Version};
use crate::error::{Error, Result};
use crate::http::hal::{HAL_CONTENT_TYPE, Hal, Links, encode};
use crate::http::method;
use crate::service::versions as service;
use crate::service::versions::{BranchVersionDetail, TagDetail, VersionDetail};

const VERSIONS_ALLOWED: &str = "GET, OPTIONS";
const VERSION_ALLOWED: &str = "GET, PUT, PATCH, DELETE, OPTIONS";
const TAG_ALLOWED: &str = "GET, PUT, DELETE, OPTIONS";
const BRANCHES_ALLOWED: &str = "GET, DELETE, OPTIONS";
const BRANCH_ALLOWED: &str = "GET, DELETE, OPTIONS";
const BRANCH_VERSION_ALLOWED: &str = "GET, PUT, DELETE, OPTIONS";
const LATEST_ALLOWED: &str = "GET, OPTIONS";

fn has_json_content_type(headers: &HeaderMap) -> bool {
    headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|ct| ct.contains("json"))
}

/// The Ruby broker's 415: a HAL content type and an empty body.
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

fn created_with_location(doc: Value, location: String) -> Response {
    let mut res = (StatusCode::CREATED, Hal(doc)).into_response();
    res.headers_mut().insert(
        header::LOCATION,
        HeaderValue::from_str(&location).expect("ascii"),
    );
    res
}

fn parse_body(body: &Bytes) -> Result<Value> {
    if body.is_empty() {
        return Ok(json!({}));
    }
    serde_json::from_slice(body).map_err(|e| Error::BadRequest(format!("JSON::ParserError - {e}")))
}

// --- hrefs ----------------------------------------------------------------------------

pub fn pacticipant_href(l: &Links, p: &str) -> String {
    l.href(&format!("/pacticipants/{}", encode(p)))
}

pub fn version_href(l: &Links, p: &str, number: &str) -> String {
    format!("{}/versions/{}", pacticipant_href(l, p), encode(number))
}

fn branch_href(l: &Links, p: &str, branch: &str) -> String {
    format!("{}/branches/{}", pacticipant_href(l, p), encode(branch))
}

// --- documents ------------------------------------------------------------------------

/// The `{number, _links: {self}}` stub embedded in pacticipant and pact documents.
pub fn version_stub(l: &Links, v: &Version) -> Value {
    json!({
        "number": v.number,
        "_links": { "self": { "title": "Version", "name": v.number, "href": version_href(l, &v.pacticipant_name, &v.number) } }
    })
}

/// A version document. `self_href` lets the latest-version routes echo their own URL;
/// the environment links appear only on single-resource responses.
pub fn version_document(
    l: &Links,
    d: &VersionDetail,
    self_href: Option<String>,
    single: bool,
) -> Value {
    let v = &d.version;
    let p = &v.pacticipant_name;
    let href = version_href(l, p, &v.number);
    let mut doc = Map::new();
    doc.insert("number".into(), json!(v.number));
    if let Some(b) = &v.build_url {
        doc.insert("buildUrl".into(), json!(b));
    }
    if let Some(u) = &v.updated_at {
        doc.insert("updatedAt".into(), json!(u));
    }
    doc.insert("createdAt".into(), json!(v.created_at));
    let branch_versions: Vec<Value> = d
        .branch_versions
        .iter()
        .map(|bv| {
            json!({
                "name": bv.branch_name,
                "latest": bv.latest,
                "_links": { "self": { "title": "Branch version", "name": bv.branch_name,
                    "href": format!("{}/versions/{}", branch_href(l, p, &bv.branch_name), encode(&v.number)) } }
            })
        })
        .collect();
    let tags: Vec<Value> = d
        .tags
        .iter()
        .map(|t| {
            json!({
                "name": t.name,
                "_links": { "self": { "title": "Tag", "name": t.name, "href": format!("{href}/tags/{}", encode(&t.name)) } }
            })
        })
        .collect();
    doc.insert(
        "_embedded".into(),
        json!({ "branchVersions": branch_versions, "tags": tags }),
    );
    let mut links = Map::new();
    links.insert("self".into(), json!({ "title": "Version", "name": v.number, "href": self_href.unwrap_or_else(|| href.clone()) }));
    links.insert(
        "pb:pacticipant".into(),
        json!({ "title": "Pacticipant", "name": p, "href": pacticipant_href(l, p) }),
    );
    links.insert("pb:tag".into(), json!({ "href": format!("{href}/tags/{{tag}}"), "title": "Get, create or delete a tag for this pacticipant version", "templated": true }));
    links.insert("pb:latest-verification-results-where-pacticipant-is-consumer".into(), json!({
        "title": "Latest verification results for consumer version",
        "href": l.href(&format!("/verification-results/consumer/{}/version/{}/latest", encode(p), encode(&v.number)))
    }));
    let pacts: Vec<Value> = d
        .providers
        .iter()
        .map(|provider| {
            json!({
                "title": "Pact",
                "name": format!("Pact between {p} ({}) and {provider}", v.number),
                "href": l.href(&format!("/pacts/provider/{}/consumer/{}/version/{}", encode(provider), encode(p), encode(&v.number)))
            })
        })
        .collect();
    links.insert("pb:pact-versions".into(), Value::Array(pacts));
    if single {
        let display = |e: &crate::db::environments::Environment| {
            e.display_name.clone().unwrap_or_else(|| e.name.clone())
        };
        let deployed: Vec<Value> = d
            .deployments
            .iter()
            .map(|(dv, e)| json!({
                "title": format!("Version deployed to {}", display(e)), "name": display(e),
                "href": l.href(&format!("/deployed-versions/{}", dv.uuid)),
                "currently_deployed": dv.currently_deployed, "deployed_at": dv.created_at, "undeployed_at": dv.undeployed_at
            }))
            .collect();
        let released: Vec<Value> = d
            .releases
            .iter()
            .map(|(rv, e)| json!({
                "title": format!("Version released to {}", display(e)), "name": display(e),
                "href": l.href(&format!("/released-versions/{}", rv.uuid)),
                "currently_supported": rv.currently_supported, "released_at": rv.created_at, "support_ended_at": rv.support_ended_at
            }))
            .collect();
        let record_deployment: Vec<Value> = d
            .environments
            .iter()
            .map(|e| json!({ "title": format!("Record deployment to {}", display(e)), "name": e.name, "href": format!("{href}/deployed-versions/environment/{}", e.uuid) }))
            .collect();
        let record_release: Vec<Value> = d
            .environments
            .iter()
            .map(|e| json!({ "title": format!("Record release to {}", display(e)), "name": e.name, "href": format!("{href}/released-versions/environment/{}", e.uuid) }))
            .collect();
        links.insert("pb:deployed-environments".into(), Value::Array(deployed));
        links.insert("pb:released-environments".into(), Value::Array(released));
        links.insert(
            "pb:record-deployment".into(),
            Value::Array(record_deployment),
        );
        links.insert("pb:record-release".into(), Value::Array(record_release));
    }
    links.insert(
        "curies".into(),
        json!([{ "name": "pb", "href": l.href("/doc/{rel}?context=version"), "templated": true }]),
    );
    doc.insert("_links".into(), Value::Object(links));
    Value::Object(doc)
}

fn tag_document(l: &Links, d: &TagDetail) -> Value {
    let v = &d.version;
    let p = &v.pacticipant_name;
    let href = version_href(l, p, &v.number);
    json!({
        "name": d.tag.name,
        "createdAt": d.tag.created_at,
        "_links": {
            "self": { "title": "Tag", "name": d.tag.name, "href": format!("{href}/tags/{}", encode(&d.tag.name)) },
            "version": { "title": "Version", "name": v.number, "href": href },
            "pacticipant": { "title": "Pacticipant", "name": p, "href": pacticipant_href(l, p) }
        }
    })
}

fn branch_document(l: &Links, p: &str, b: &Branch) -> Value {
    let href = branch_href(l, p, &b.name);
    let mut doc = Map::new();
    doc.insert("name".into(), json!(b.name));
    // The Ruby broker omits a branch's updatedAt when it equals createdAt (verified).
    if let Some(u) = &b.updated_at
        && u != &b.created_at
    {
        doc.insert("updatedAt".into(), json!(u));
    }
    doc.insert("createdAt".into(), json!(b.created_at));
    doc.insert("_links".into(), json!({
        "self": { "title": "Branch", "href": href },
        "pb:latest-version": { "title": "Latest version for branch", "href": format!("{href}/latest-version") }
    }));
    Value::Object(doc)
}

fn branch_version_document(l: &Links, d: &BranchVersionDetail) -> Value {
    let p = &d.version.pacticipant_name;
    let bhref = branch_href(l, p, &d.branch.name);
    json!({
        "createdAt": d.created_at,
        "_links": {
            "self": { "title": "Branch version", "href": format!("{bhref}/versions/{}", encode(&d.version.number)) },
            "pb:branch": { "title": "Branch", "name": d.branch.name, "href": bhref },
            "pb:version": { "title": "Version", "name": d.version.number, "href": version_href(l, p, &d.version.number) }
        }
    })
}

/// `dd/mm/yyyy` from a stored timestamp, as the Ruby broker titles version links.
fn short_date(created_at: &str) -> String {
    match created_at.get(0..10) {
        Some(ymd) if ymd.len() == 10 => format!("{}/{}/{}", &ymd[8..10], &ymd[5..7], &ymd[0..4]),
        _ => created_at.to_owned(),
    }
}

// --- handlers -------------------------------------------------------------------------

/// `GET /pacticipants/{p}/versions`
pub async fn versions(
    m: Method,
    State(state): State<AppState>,
    Path(p): Path<String>,
    l: Links,
) -> Result<Response> {
    match m {
        Method::GET => {
            let (pacticipant, versions) = service::list(&state.db, &p).await?;
            let base = pacticipant_href(&l, &pacticipant.name);
            let embedded: Vec<Value> = versions
                .iter()
                .map(|d| version_document(&l, d, None, false))
                .collect();
            let links: Vec<Value> = versions
                .iter()
                .map(|d| json!({ "href": version_href(&l, &p, &d.version.number), "title": format!("Version {} - {}", d.version.number, short_date(&d.version.created_at)) }))
                .collect();
            let deprecated: Vec<Value> = versions
                .iter()
                .map(|d| json!({ "href": version_href(&l, &p, &d.version.number), "title": "Deprecated - please use pb:versions" }))
                .collect();
            Ok(Hal(json!({
                "_embedded": { "versions": embedded },
                "_links": {
                    "self": { "href": format!("{base}/versions"), "title": format!("All application versions of {}", pacticipant.name) },
                    "pb:pacticipant": { "href": base, "title": pacticipant.name },
                    "pb:versions": links,
                    "pacticipant": { "href": base, "title": "Deprecated - please use pb:pacticipant" },
                    "versions": deprecated
                }
            }))
            .into_response())
        }
        Method::OPTIONS => Ok(method::options(VERSIONS_ALLOWED)),
        _ => Ok(method::method_not_allowed(VERSIONS_ALLOWED)),
    }
}

/// `GET | PUT | PATCH | DELETE /pacticipants/{p}/versions/{v}`
pub async fn version(
    m: Method,
    State(state): State<AppState>,
    Path((p, v)): Path<(String, String)>,
    l: Links,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response> {
    match m {
        Method::GET => {
            let d = service::find(&state.db, &p, &v).await?;
            Ok(Hal(version_document(&l, &d, None, true)).into_response())
        }
        Method::PUT | Method::PATCH => {
            if !has_json_content_type(&headers) {
                return Ok(unsupported_media_type());
            }
            let json = parse_body(&body)?;
            let build_url = match json.get("buildUrl") {
                None | Some(Value::Null) => None,
                Some(Value::String(s)) if s.is_empty() => Some(None),
                Some(Value::String(s)) => Some(Some(s.clone())),
                Some(other) => Some(Some(other.to_string())),
            };
            let (d, created) =
                service::put(&state.db, &p, &v, build_url.as_ref().map(|o| o.as_deref())).await?;
            let status = if created {
                StatusCode::CREATED
            } else {
                StatusCode::OK
            };
            Ok((status, Hal(version_document(&l, &d, None, true))).into_response())
        }
        Method::DELETE => {
            service::delete(&state.db, &p, &v).await?;
            Ok(StatusCode::NO_CONTENT.into_response())
        }
        Method::OPTIONS => Ok(method::options(VERSION_ALLOWED)),
        _ => Ok(method::method_not_allowed(VERSION_ALLOWED)),
    }
}

/// `GET | PUT | DELETE /pacticipants/{p}/versions/{v}/tags/{t}`
pub async fn tag(
    m: Method,
    State(state): State<AppState>,
    Path((p, v, t)): Path<(String, String, String)>,
    l: Links,
    headers: HeaderMap,
) -> Result<Response> {
    match m {
        Method::GET => {
            let d = service::find_tag(&state.db, &p, &v, &t).await?;
            Ok(Hal(tag_document(&l, &d)).into_response())
        }
        Method::PUT => {
            if !has_json_content_type(&headers) {
                return Ok(unsupported_media_type());
            }
            let (d, created) = service::put_tag(&state.db, &p, &v, &t).await?;
            let doc = tag_document(&l, &d);
            if created {
                let location = format!("{}/tags/{}", version_href(&l, &p, &v), encode(&t));
                Ok(created_with_location(doc, location))
            } else {
                Ok(Hal(doc).into_response())
            }
        }
        Method::DELETE => {
            service::delete_tag(&state.db, &p, &v, &t).await?;
            Ok(StatusCode::NO_CONTENT.into_response())
        }
        Method::OPTIONS => Ok(method::options(TAG_ALLOWED)),
        _ => Ok(method::method_not_allowed(TAG_ALLOWED)),
    }
}

/// The `exclude[]=name` repeats of a bulk branch delete. serde_urlencoded cannot read a
/// repeated key into a Vec, so the query string is split by hand.
fn excluded_branches(query: Option<&str>) -> Vec<String> {
    query
        .unwrap_or_default()
        .split('&')
        .filter_map(|pair| {
            let (k, v) = pair.split_once('=')?;
            let k = percent_decode(k);
            (k == "exclude[]" || k == "exclude").then(|| percent_decode(v))
        })
        .collect()
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(hex) = s.get(i + 1..i + 3)
            && let Ok(b) = u8::from_str_radix(hex, 16)
        {
            out.push(b);
            i += 3;
        } else if bytes[i] == b'+' {
            out.push(b' ');
            i += 1;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `GET | DELETE /pacticipants/{p}/branches`
pub async fn branches(
    m: Method,
    State(state): State<AppState>,
    Path(p): Path<String>,
    RawQuery(query): RawQuery,
    l: Links,
) -> Result<Response> {
    match m {
        Method::GET => {
            let branches = service::list_branches(&state.db, &p).await?;
            let embedded: Vec<Value> = branches
                .iter()
                .map(|b| branch_document(&l, &p, b))
                .collect();
            let links: Vec<Value> = branches
                .iter()
                .map(|b| json!({ "name": b.name, "href": branch_href(&l, &p, &b.name) }))
                .collect();
            Ok(Hal(json!({
                "page": { "number": 1, "size": 100, "totalElements": branches.len(), "totalPages": 1 },
                "_embedded": { "branches": embedded },
                "_links": {
                    "self": { "title": format!("{p} branches"), "href": format!("{}/branches", pacticipant_href(&l, &p)) },
                    "pb:branches": links
                }
            }))
            .into_response())
        }
        Method::DELETE => {
            // The Ruby broker deletes asynchronously and answers 202 with a notice
            // (verified); pactling deletes before answering with the same shape.
            let keep = excluded_branches(query.as_deref());
            let (deleted, remaining) =
                service::delete_branches_except(&state.db, &p, &keep).await?;
            let names: Vec<&str> = remaining.iter().map(|b| b.name.as_str()).collect();
            Ok((
                StatusCode::ACCEPTED,
                Hal(json!({ "notices": [{ "type": "success", "text": format!("Scheduled deletion of {deleted} branches for pacticipant {p}. Remaining branches are: {}", names.join(", ")) }] })),
            )
                .into_response())
        }
        Method::OPTIONS => Ok(method::options(BRANCHES_ALLOWED)),
        _ => Ok(method::method_not_allowed(BRANCHES_ALLOWED)),
    }
}

/// `GET | DELETE /pacticipants/{p}/branches/{b}`
pub async fn branch(
    m: Method,
    State(state): State<AppState>,
    Path((p, b)): Path<(String, String)>,
    l: Links,
) -> Result<Response> {
    match m {
        Method::GET => {
            let branch = service::find_branch(&state.db, &p, &b).await?;
            Ok(Hal(branch_document(&l, &p, &branch)).into_response())
        }
        Method::DELETE => {
            service::delete_branch(&state.db, &p, &b).await?;
            Ok(StatusCode::NO_CONTENT.into_response())
        }
        Method::OPTIONS => Ok(method::options(BRANCH_ALLOWED)),
        _ => Ok(method::method_not_allowed(BRANCH_ALLOWED)),
    }
}

/// `GET | PUT | DELETE /pacticipants/{p}/branches/{b}/versions/{v}`
pub async fn branch_version(
    m: Method,
    State(state): State<AppState>,
    Path((p, b, v)): Path<(String, String, String)>,
    l: Links,
    headers: HeaderMap,
) -> Result<Response> {
    match m {
        Method::GET => {
            let d = service::find_branch_version(&state.db, &p, &b, &v).await?;
            Ok(Hal(branch_version_document(&l, &d)).into_response())
        }
        Method::PUT => {
            if !has_json_content_type(&headers) {
                return Ok(unsupported_media_type());
            }
            let (d, created) = service::put_branch_version(&state.db, &p, &b, &v).await?;
            let doc = branch_version_document(&l, &d);
            if created {
                let location = format!("{}/versions/{}", branch_href(&l, &p, &b), encode(&v));
                Ok(created_with_location(doc, location))
            } else {
                Ok(Hal(doc).into_response())
            }
        }
        Method::DELETE => {
            service::delete_branch_version(&state.db, &p, &b, &v).await?;
            Ok(StatusCode::NO_CONTENT.into_response())
        }
        Method::OPTIONS => Ok(method::options(BRANCH_VERSION_ALLOWED)),
        _ => Ok(method::method_not_allowed(BRANCH_VERSION_ALLOWED)),
    }
}

/// `GET /pacticipants/{p}/branches/{b}/latest-version`
pub async fn branch_latest_version(
    m: Method,
    State(state): State<AppState>,
    Path((p, b)): Path<(String, String)>,
    l: Links,
) -> Result<Response> {
    match m {
        Method::GET => {
            let d = service::latest_in_branch(&state.db, &p, &b).await?;
            let self_href = format!("{}/latest-version", branch_href(&l, &p, &b));
            Ok(Hal(version_document(&l, &d, Some(self_href), true)).into_response())
        }
        Method::OPTIONS => Ok(method::options(LATEST_ALLOWED)),
        _ => Ok(method::method_not_allowed(LATEST_ALLOWED)),
    }
}

/// `GET /pacticipants/{p}/latest-version`
pub async fn latest_version(
    m: Method,
    State(state): State<AppState>,
    Path(p): Path<String>,
    l: Links,
) -> Result<Response> {
    match m {
        Method::GET => {
            let d = service::latest(&state.db, &p).await?;
            let self_href = format!("{}/latest-version", pacticipant_href(&l, &p));
            Ok(Hal(version_document(&l, &d, Some(self_href), true)).into_response())
        }
        Method::OPTIONS => Ok(method::options(LATEST_ALLOWED)),
        _ => Ok(method::method_not_allowed(LATEST_ALLOWED)),
    }
}

/// `GET /pacticipants/{p}/latest-version/{tag}`
pub async fn latest_version_with_tag(
    m: Method,
    State(state): State<AppState>,
    Path((p, t)): Path<(String, String)>,
    l: Links,
) -> Result<Response> {
    match m {
        Method::GET => {
            let d = service::latest_with_tag(&state.db, &p, &t).await?;
            let self_href = format!("{}/latest-version/{}", pacticipant_href(&l, &p), encode(&t));
            Ok(Hal(version_document(&l, &d, Some(self_href), true)).into_response())
        }
        Method::OPTIONS => Ok(method::options(LATEST_ALLOWED)),
        _ => Ok(method::method_not_allowed(LATEST_ALLOWED)),
    }
}
