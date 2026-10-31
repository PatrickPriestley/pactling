use crate::error::Result;
use crate::http::hal::{Hal, Links};
use serde_json::json;

pub async fn list(l: Links) -> Result<Hal> {
    Ok(Hal(json!({
        "_embedded": { "integrations": [] },
        "_links": {
            "self": { "href": l.href("/integrations"), "title": "All integrations" }
        }
    })))
}
