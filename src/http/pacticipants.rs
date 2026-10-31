//! `/pacticipants` and `/pacticipants/{name}`. Documents are rendered to match the
//! captured fixtures; see tests/pacticipants.rs for the verified behaviour.
use axum::{
    body::Bytes,
    extract::{Path, State},
    http::{HeaderValue, Method, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde_json::{Value, json};

use crate::app::AppState;
use crate::db::pacticipants::Pacticipant;
use crate::error::{Error, Result};
use crate::http::hal::{Hal, Links, encode};
use crate::http::method;
use crate::service::pacticipants as service;
use crate::service::pacticipants::Patch;

const COLLECTION_ALLOWED: &str = "GET, POST, OPTIONS";
const RESOURCE_ALLOWED: &str = "GET, PUT, PATCH, DELETE, OPTIONS";

fn parse_body(body: &Bytes) -> Result<Value> {
    if body.is_empty() {
        return Ok(json!({}));
    }
    serde_json::from_slice(body).map_err(|e| Error::BadRequest(format!("JSON::ParserError - {e}")))
}

/// `GET`, `POST`, `OPTIONS` on the collection; anything else is 405.
pub async fn collection(
    m: Method,
    State(state): State<AppState>,
    l: Links,
    body: Bytes,
) -> Result<Response> {
    match m {
        Method::GET => Ok(list(&state, &l).await?.into_response()),
        Method::POST => {
            let json = parse_body(&body)?;
            let patch = Patch::from_json(&json);
            let name = patch.validate_for_create()?.to_owned();
            let saved = service::upsert(&state.db, &name, &patch).await?;
            let mut res = (
                StatusCode::CREATED,
                Hal(document(&saved.pacticipant, &l, false)),
            )
                .into_response();
            // The Ruby broker's Location header really is spelled "pacticpants".
            let location = l.href(&format!("/pacticpants/{}", encode(&name)));
            res.headers_mut().insert(
                header::LOCATION,
                HeaderValue::from_str(&location).expect("ascii"),
            );
            Ok(res)
        }
        Method::OPTIONS => Ok(method::options(COLLECTION_ALLOWED)),
        _ => Ok(method::method_not_allowed(COLLECTION_ALLOWED)),
    }
}

/// `GET`, `PUT`, `PATCH`, `DELETE`, `OPTIONS` on one pacticipant. Every document rendered
/// here carries `pb:deployed-environments`; collection responses do not (verified).
pub async fn resource(
    m: Method,
    State(state): State<AppState>,
    Path(name): Path<String>,
    l: Links,
    body: Bytes,
) -> Result<Response> {
    match m {
        Method::GET => {
            let p = service::find(&state.db, &name).await?;
            Ok(Hal(document(&p, &l, true)).into_response())
        }
        Method::PATCH => {
            let patch = Patch::from_json(&parse_body(&body)?);
            let saved = service::upsert(&state.db, &name, &patch).await?;
            let mut res = Hal(document(&saved.pacticipant, &l, true)).into_response();
            if saved.created {
                // Unlike POST, the PATCH Location is spelled correctly (verified).
                *res.status_mut() = StatusCode::CREATED;
                let location = l.href(&format!("/pacticipants/{}", encode(&name)));
                res.headers_mut().insert(
                    header::LOCATION,
                    HeaderValue::from_str(&location).expect("ascii"),
                );
            }
            Ok(res)
        }
        Method::PUT => {
            let patch = Patch::from_json(&parse_body(&body)?);
            let p = service::update(&state.db, &name, &patch).await?;
            Ok(Hal(document(&p, &l, true)).into_response())
        }
        Method::DELETE => {
            service::delete(&state.db, &name).await?;
            Ok(StatusCode::NO_CONTENT.into_response())
        }
        Method::OPTIONS => Ok(method::options(RESOURCE_ALLOWED)),
        _ => Ok(method::method_not_allowed(RESOURCE_ALLOWED)),
    }
}

async fn list(state: &AppState, l: &Links) -> Result<Hal> {
    let all = state.db.list_pacticipants().await?;
    let embedded: Vec<Value> = all.iter().map(|p| document(p, l, false)).collect();
    let links: Vec<Value> = all
        .iter()
        .map(|p| json!({ "href": self_href(p, l), "title": "Pacticipant", "name": p.name }))
        .collect();
    let deprecated_links: Vec<Value> = all
        .iter()
        .map(|p| json!({ "href": self_href(p, l), "title": p.name, "name": "DEPRECATED - please use pb:pacticipants" }))
        .collect();
    let deprecated_docs: Vec<Value> = all
        .iter()
        .map(|p| {
            let mut doc = document(p, l, false);
            let obj = doc.as_object_mut().unwrap();
            obj.shift_remove("_embedded");
            obj.shift_remove("_links");
            let mut with_title = doc.clone();
            with_title.as_object_mut().unwrap().insert(
                "title".into(),
                json!("DEPRECATED - Please use the embedded pacticipants collection"),
            );
            let full = document(p, l, false);
            with_title["_embedded"] = full["_embedded"].clone();
            with_title["_links"] = full["_links"].clone();
            with_title
        })
        .collect();
    Ok(Hal(json!({
        "_embedded": { "pacticipants": embedded },
        "_links": {
            "self": { "href": l.href("/pacticipants") },
            "pb:pacticipants-with-label": l.link("/pacticipants/label/{label}", "Find pacticipants by label", true),
            "pb:pacticipants": links,
            "pacticipants": deprecated_links
        },
        "pacticipants": deprecated_docs
    })))
}

fn self_href(p: &Pacticipant, l: &Links) -> String {
    l.href(&format!("/pacticipants/{}", encode(&p.name)))
}

/// One pacticipant document. Only the single-resource `GET` carries
/// `pb:deployed-environments`; `POST`, `PATCH` and list entries do not.
pub fn document(p: &Pacticipant, l: &Links, with_deployed_environments: bool) -> Value {
    let base = self_href(p, l);
    let n = &p.name;
    let mut doc = serde_json::Map::new();
    doc.insert("name".into(), json!(n));
    if let Some(d) = &p.display_name {
        doc.insert("displayName".into(), json!(d));
    }
    if let Some(r) = &p.repository_url {
        doc.insert("repositoryUrl".into(), json!(r));
    }
    if let Some(b) = &p.main_branch {
        doc.insert("mainBranch".into(), json!(b));
    }
    if let Some(u) = &p.updated_at {
        doc.insert("updatedAt".into(), json!(u));
    }
    doc.insert("createdAt".into(), json!(p.created_at));
    doc.insert("_embedded".into(), json!({ "labels": [] }));
    let mut links = serde_json::Map::new();
    links.insert("self".into(), json!({ "href": base }));
    links.insert(
        "pb:versions".into(),
        json!({ "href": format!("{base}/versions") }),
    );
    links.insert(
        "pb:branches".into(),
        json!({ "href": format!("{base}/branches") }),
    );
    if with_deployed_environments {
        links.insert("pb:deployed-environments".into(), json!([]));
    }
    links.insert("pb:version".into(), json!({ "title": "Get, create or delete a pacticipant version", "href": format!("{base}/versions/{{version}}"), "templated": true }));
    links.insert("pb:version-tag".into(), json!({ "title": format!("Get, create or delete a tag for a version of {n}"), "href": format!("{base}/versions/{{version}}/tags/{{tag}}"), "templated": true }));
    links.insert("pb:branch-version".into(), json!({ "title": format!("Get or add/create a version for a branch of {n}"), "href": format!("{base}/branches/{{branch}}/versions/{{version}}"), "templated": true }));
    links.insert("pb:label".into(), json!({ "title": format!("Get, create or delete a label for {n}"), "href": format!("{base}/labels/{{label}}"), "templated": true }));
    links.insert(
        "versions".into(),
        json!({ "title": "Deprecated - use pb:versions", "href": format!("{base}/versions") }),
    );
    links.insert("pb:can-i-deploy-badge".into(), json!({ "title": format!("Can I Deploy {n} badge"), "href": format!("{base}/latest-version/{{tag}}/can-i-deploy/to/{{environmentTag}}/badge"), "templated": true }));
    links.insert("pb:can-i-deploy-branch-to-environment-badge".into(), json!({ "title": format!("Can I Deploy {n} from branch to environment badge"), "href": format!("{base}/branches/{{branch}}/latest-version/can-i-deploy/to-environment/{{environment}}/badge"), "templated": true }));
    links.insert("curies".into(), json!([{ "name": "pb", "href": l.href("/doc/{rel}?context=pacticipant"), "templated": true }]));
    doc.insert("_links".into(), Value::Object(links));
    Value::Object(doc)
}
