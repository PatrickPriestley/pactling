//! Webmachine-style method handling: anything not explicitly allowed is 405 with `allow`.
use axum::{
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};

pub fn method_not_allowed(allowed: &'static str) -> Response {
    let mut res = StatusCode::METHOD_NOT_ALLOWED.into_response();
    res.headers_mut()
        .insert(header::ALLOW, HeaderValue::from_static(allowed));
    res
}

pub fn options(allowed: &'static str) -> Response {
    let mut res = StatusCode::NO_CONTENT.into_response();
    res.headers_mut()
        .insert(header::ALLOW, HeaderValue::from_static(allowed));
    res
}
