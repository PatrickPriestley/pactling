//! `/matrix`, `/matrix/provider/{p}/consumer/{c}` and the index's `/can-i-deploy`
//! relation. The document shape follows the Phase 4 capture (tests/matrix.rs).
use axum::{
    extract::{Path, RawQuery, State},
    http::Method,
    response::{IntoResponse, Response},
};
use serde_json::{Map, Value, json};

use crate::app::AppState;
use crate::db::versions::Version;
use crate::error::{Error, Result};
use crate::http::environments::environment_stub;
use crate::http::hal::{Hal, Links, encode};
use crate::http::method;
use crate::http::pacts::pact_href;
use crate::http::versions::{pacticipant_href, version_href};
use crate::service::matrix::{self as service, Matrix, MatrixQuery, Row, Selector};

const ALLOWED: &str = "GET, OPTIONS";

/// `GET /matrix?...`
pub async fn matrix(
    m: Method,
    State(state): State<AppState>,
    RawQuery(query): RawQuery,
    l: Links,
) -> Result<Response> {
    match m {
        Method::GET => {
            let q = MatrixQuery::parse(query.as_deref());
            let result = service::matrix(&state.db, &q).await?;
            Ok(Hal(document(&state, &l, &result).await?).into_response())
        }
        Method::OPTIONS => Ok(method::options(ALLOWED)),
        _ => Ok(method::method_not_allowed(ALLOWED)),
    }
}

/// `GET /can-i-deploy?pacticipant=&version=&environment=|to=`: the same document, with
/// the Ruby broker's field errors when parameters are missing (verified in Phase 1).
pub async fn can_i_deploy(
    m: Method,
    State(state): State<AppState>,
    RawQuery(query): RawQuery,
    l: Links,
) -> Result<Response> {
    match m {
        Method::GET => {
            let q = MatrixQuery::parse(query.as_deref());
            let mut errors = Map::new();
            let selector = q.selectors.first();
            if selector.is_none() {
                errors.insert("pacticipant".into(), json!(["is missing"]));
            }
            if selector.is_none_or(|s| s.version.is_none()) {
                errors.insert("version".into(), json!(["is missing"]));
            }
            if q.environment.is_none() && q.tag.is_none() {
                errors.insert(
                    "".into(),
                    json!(["Must specify either an environment or a 'to' tag."]),
                );
            }
            if !errors.is_empty() {
                return Err(Error::Validation(Value::Object(errors)));
            }
            let result = service::matrix(&state.db, &q).await?;
            Ok(Hal(document(&state, &l, &result).await?).into_response())
        }
        Method::OPTIONS => Ok(method::options(ALLOWED)),
        _ => Ok(method::method_not_allowed(ALLOWED)),
    }
}

/// `GET /matrix/provider/{p}/consumer/{c}`: every row of the integration.
pub async fn integration(
    m: Method,
    State(state): State<AppState>,
    Path((provider, consumer)): Path<(String, String)>,
    l: Links,
) -> Result<Response> {
    match m {
        Method::GET => {
            let q = MatrixQuery {
                selectors: vec![
                    Selector {
                        pacticipant: consumer,
                        ..Default::default()
                    },
                    Selector {
                        pacticipant: provider,
                        ..Default::default()
                    },
                ],
                ..Default::default()
            };
            let result = service::matrix(&state.db, &q).await?;
            Ok(Hal(document(&state, &l, &result).await?).into_response())
        }
        Method::OPTIONS => Ok(method::options(ALLOWED)),
        _ => Ok(method::method_not_allowed(ALLOWED)),
    }
}

async fn document(state: &AppState, l: &Links, r: &Matrix) -> Result<Value> {
    let mut rows = Vec::with_capacity(r.rows.len());
    for row in &r.rows {
        rows.push(row_document(state, l, row).await?);
    }
    let mut summary = Map::new();
    summary.insert("deployable".into(), json!(r.deployable));
    summary.insert(
        "reason".into(),
        json!(
            r.notices
                .iter()
                .map(|n| n.text.as_str())
                .collect::<Vec<_>>()
                .join("\n")
        ),
    );
    summary.insert("success".into(), json!(r.success));
    summary.insert("failed".into(), json!(r.failed));
    summary.insert("unknown".into(), json!(r.unknown));
    if r.has_ignores {
        summary.insert("ignored".into(), json!(r.ignored));
    }
    let notices: Vec<Value> = r
        .notices
        .iter()
        .map(|n| json!({ "type": n.kind, "text": n.text }))
        .collect();
    Ok(json!({ "summary": summary, "notices": notices, "matrix": rows }))
}

async fn version_document(state: &AppState, l: &Links, v: &Version) -> Result<Value> {
    let branches = state.db.branch_versions_for_version(v.id).await?;
    let branch_docs: Vec<Value> = branches
        .iter()
        .map(|b| json!({ "name": b.branch_name, "latest": b.latest, "_links": { "self": { "title": "Branch version", "name": b.branch_name, "href": format!("{}/branches/{}/versions/{}", pacticipant_href(l, &v.pacticipant_name), encode(&b.branch_name), encode(&v.number)) } } }))
        .collect();
    let mut envs = vec![];
    for e in state.db.environments_of_version(v.id).await? {
        envs.push(environment_stub(l, &e));
    }
    let mut tags = vec![];
    for t in state.db.tags_for_version(v.id).await? {
        let latest = state.db.is_latest_with_tag(v, &t.name).await?;
        tags.push(json!({ "name": t.name, "latest": latest, "_links": { "self": { "href": format!("{}/tags/{}", version_href(l, &v.pacticipant_name, &v.number), encode(&t.name)) } } }));
    }
    let mut doc = Map::new();
    doc.insert("number".into(), json!(v.number));
    if let Some(first) = branches.first() {
        doc.insert("branch".into(), json!(first.branch_name));
    }
    doc.insert("branches".into(), json!(branch_docs));
    doc.insert("branchVersions".into(), json!(branch_docs));
    doc.insert("environments".into(), json!(envs));
    doc.insert(
        "_links".into(),
        json!({ "self": { "href": version_href(l, &v.pacticipant_name, &v.number) } }),
    );
    doc.insert("tags".into(), json!(tags));
    Ok(Value::Object(doc))
}

async fn row_document(state: &AppState, l: &Links, row: &Row) -> Result<Value> {
    let consumer_version = match &row.consumer.version {
        Some(v) => version_document(state, l, v).await?,
        None => Value::Null,
    };
    let provider_version = match &row.provider.version {
        Some(v) => version_document(state, l, v).await?,
        None => Value::Null,
    };
    let verification = match &row.verification {
        Some(vf) => {
            let base = l.href(&format!(
                "/pacts/provider/{}/consumer/{}/pact-version/{}",
                encode(&row.pact.provider_name),
                encode(&row.pact.consumer_name),
                row.pact.sha
            ));
            let metadata = base64_url(&format!("cvn={}", row.pact.consumer_version_number));
            json!({ "success": vf.success, "verifiedAt": vf.created_at, "_links": { "self": { "href": format!("{base}/metadata/{metadata}/verification-results/{}", vf.number) } } })
        }
        None => Value::Null,
    };
    let mut doc = Map::new();
    doc.insert("consumer".into(), json!({ "name": row.consumer.pacticipant.name, "version": consumer_version, "_links": { "self": { "href": pacticipant_href(l, &row.consumer.pacticipant.name) } } }));
    doc.insert("provider".into(), json!({ "name": row.provider.pacticipant.name, "version": provider_version, "_links": { "self": { "href": pacticipant_href(l, &row.provider.pacticipant.name) } } }));
    doc.insert("pact".into(), json!({ "createdAt": row.pact.created_at, "_links": { "self": { "href": pact_href(l, &row.pact.provider_name, &row.pact.consumer_name, &row.pact.consumer_version_number) } } }));
    doc.insert("verificationResult".into(), verification);
    if row.ignored {
        doc.insert("ignored".into(), json!(true));
    }
    Ok(Value::Object(doc))
}

fn base64_url(s: &str) -> String {
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(s)
}
