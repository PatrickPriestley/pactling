use crate::app::AppState;
use crate::error::{Error, Result};
use crate::http::hal::{Hal, Links};
use axum::extract::{Path, State};
use serde_json::json;

pub async fn list(State(state): State<AppState>, l: Links) -> Result<Hal> {
    let names = state.db.list_pacticipant_names().await?;
    let embedded: Vec<serde_json::Value> = names.iter().map(|n| json!({ "name": n })).collect();
    let links: Vec<serde_json::Value> = names
        .iter()
        .map(|n| json!({ "href": l.href(&format!("/pacticipants/{n}")), "title": n }))
        .collect();
    Ok(Hal(json!({
        "_embedded": { "pacticipants": embedded },
        "_links": {
            "self": { "href": l.href("/pacticipants") },
            "pb:pacticipants-with-label": l.link("/pacticipants/label/{label}", "Find pacticipants by label", true),
            "pb:pacticipants": links,
            "pacticipants": links
        },
        "pacticipants": links
    })))
}

pub async fn get_pacticipant(Path(name): Path<String>) -> Result<Hal> {
    Err(Error::NotFound(format!(
        "No pacticipant with name '{name}' found"
    )))
}
