//! `/matrix` and `/can-i-deploy`. Phase 1 reproduces only the Ruby broker's validation
//! errors for missing parameters; evaluation arrives in Phase 4.
use crate::error::{Error, Result};
use crate::http::hal::Hal;
use axum::extract::RawQuery;
use serde_json::json;
use std::collections::HashMap;

fn params(raw: Option<String>) -> HashMap<String, String> {
    raw.map(|q| {
        q.split('&')
            .filter(|p| !p.is_empty())
            .map(|p| {
                let (k, v) = p.split_once('=').unwrap_or((p, ""));
                (k.to_string(), v.to_string())
            })
            .collect()
    })
    .unwrap_or_default()
}

pub async fn matrix(RawQuery(raw): RawQuery) -> Result<Hal> {
    let p = params(raw);
    if !p.keys().any(|k| k.starts_with("q[")) {
        return Err(Error::Validation(json!([
            "Please provide 1 or more version selectors."
        ])));
    }
    Err(Error::Validation(json!([
        "Matrix evaluation is not implemented yet"
    ])))
}

pub async fn can_i_deploy(RawQuery(raw): RawQuery) -> Result<Hal> {
    let p = params(raw);
    let mut errors = serde_json::Map::new();
    for required in ["pacticipant", "version"] {
        if !p.contains_key(required) {
            errors.insert(required.into(), json!(["is missing"]));
        }
    }
    if !p.contains_key("environment") && !p.contains_key("to") {
        errors.insert(
            "".into(),
            json!(["Must specify either an environment or a 'to' tag."]),
        );
    }
    if !errors.is_empty() {
        return Err(Error::Validation(serde_json::Value::Object(errors)));
    }
    Err(Error::Validation(json!([
        "can-i-deploy is not implemented yet"
    ])))
}
