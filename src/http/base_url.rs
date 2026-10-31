//! Derives the base URL for links from the request, as the Ruby broker does from `Host`.
use axum::{
    extract::FromRequestParts,
    http::{StatusCode, request::Parts},
};

use super::hal::Links;

impl<S: Send + Sync> FromRequestParts<S> for Links {
    type Rejection = StatusCode;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let host = parts
            .headers
            .get("host")
            .and_then(|h| h.to_str().ok())
            .ok_or(StatusCode::BAD_REQUEST)?;
        let scheme = parts
            .headers
            .get("x-forwarded-proto")
            .and_then(|h| h.to_str().ok())
            .unwrap_or("http");
        Ok(Links::new(format!("{scheme}://{host}")))
    }
}
