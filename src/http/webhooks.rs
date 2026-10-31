use crate::error::Result;
use crate::http::hal::{Hal, Links};
use serde_json::json;

pub async fn list(l: Links) -> Result<Hal> {
    Ok(Hal(json!({
        "_links": {
            "self": { "title": "Webhooks", "href": l.href("/webhooks") },
            "pb:create": { "title": "POST to create a webhook", "href": l.href("/webhooks") },
            "pb:webhooks": [],
            "curies": [{ "name": "pb", "href": l.href("/doc/webhooks-{rel}"), "templated": true }]
        }
    })))
}
