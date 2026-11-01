//! Additive read-only HTML views; compatibility routes keep their representations.
pub mod index;
pub mod layout;

use axum::{
    http::{Method, header},
    response::{IntoResponse, Response},
};

pub const ALLOWED: &str = "GET, OPTIONS";

pub async fn style(method: Method) -> Response {
    match method {
        Method::GET => (
            [(header::CONTENT_TYPE, "text/css; charset=utf-8")],
            include_str!("style.css"),
        )
            .into_response(),
        Method::OPTIONS => super::method::options(ALLOWED),
        _ => super::method::method_not_allowed(ALLOWED),
    }
}

pub mod links;
