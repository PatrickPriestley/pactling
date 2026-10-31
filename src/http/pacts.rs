use crate::error::Result;
use crate::http::hal::{Hal, Links};
use serde_json::json;

pub async fn latest(l: Links) -> Result<Hal> {
    Ok(Hal(json!({
        "pacts": [],
        "_links": {
            "self": { "href": l.href("/pacts/latest") },
            "pb:pacts": [],
            "pacts": [],
            "curies": [{ "name": "pb", "href": l.href("/doc/{rel}?context=pacts"), "templated": true }]
        }
    })))
}
