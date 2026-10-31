use axum::{
    http::{HeaderValue, header},
    response::{IntoResponse, Response},
};
use serde_json::{Value, json};

pub const HAL_CONTENT_TYPE: &str = "application/hal+json;charset=utf-8";
pub const JSON_CONTENT_TYPE: &str = "application/json;charset=utf-8";

/// A HAL document. Serialises with `content-type: application/hal+json;charset=utf-8`.
pub struct Hal(pub Value);

impl IntoResponse for Hal {
    fn into_response(self) -> Response {
        let mut res = axum::Json(self.0).into_response();
        res.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static(HAL_CONTENT_TYPE),
        );
        res
    }
}

/// Builds absolute hrefs from the request's base URL, as the Ruby broker does.
#[derive(Clone, Debug)]
pub struct Links {
    base: String,
}

impl Links {
    pub fn new(base: impl Into<String>) -> Self {
        Self { base: base.into() }
    }

    pub fn href(&self, path: &str) -> String {
        format!("{}{}", self.base, path)
    }

    pub fn link(&self, path: &str, title: &str, templated: bool) -> Value {
        json!({ "href": self.href(path), "title": title, "templated": templated })
    }
}

/// Percent-encodes one path segment the way the Ruby broker's links do: everything but
/// unreserved characters is escaped, so `Example App` becomes `Example%20App`.
pub fn encode(segment: &str) -> String {
    let mut out = String::with_capacity(segment.len());
    for b in segment.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}
