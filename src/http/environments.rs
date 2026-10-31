use crate::error::Result;
use crate::http::hal::{Hal, Links};
use serde_json::json;

pub async fn list(l: Links) -> Result<Hal> {
    Ok(Hal(json!({
        "_embedded": { "environments": [] },
        "_links": {
            "self": { "title": "Environments", "href": l.href("/environments") },
            "pb:environments": []
        }
    })))
}
