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

/// The Ruby broker answers OPTIONS with 200, an empty body and the methods in
/// `access-control-allow-methods` (verified; it does not send `allow` here).
pub fn options(allowed: &'static str) -> Response {
    let mut res = StatusCode::OK.into_response();
    res.headers_mut().insert(
        header::ACCESS_CONTROL_ALLOW_METHODS,
        HeaderValue::from_static(allowed),
    );
    res
}
