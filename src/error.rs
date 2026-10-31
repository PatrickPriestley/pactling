use axum::{
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde_json::json;

use crate::http::hal::{HAL_CONTENT_TYPE, JSON_CONTENT_TYPE};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A route exists but the addressed resource does not. HAL, with the message.
    #[error("{0}")]
    NotFound(String),
    /// No route matched. Plain JSON with the Ruby broker's generic message.
    #[error("The requested document was not found on this server.")]
    NoRoute,
    /// Bad input. The value is the Ruby broker's `errors` member (array or object).
    #[error("{0}")]
    Validation(serde_json::Value),
    /// Unparseable input. Rendered as the Ruby broker's `{"error": "..."}` with status 400.
    #[error("{0}")]
    BadRequest(String),
    #[error(transparent)]
    Db(#[from] sqlx::Error),
}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        let (status, content_type, body) = match &self {
            Error::NotFound(msg) => (
                StatusCode::NOT_FOUND,
                HAL_CONTENT_TYPE,
                json!({ "error": msg }),
            ),
            Error::NoRoute => (
                StatusCode::NOT_FOUND,
                JSON_CONTENT_TYPE,
                json!({ "error": self.to_string() }),
            ),
            Error::Validation(errors) => (
                StatusCode::BAD_REQUEST,
                HAL_CONTENT_TYPE,
                json!({ "errors": errors }),
            ),
            Error::BadRequest(msg) => (
                StatusCode::BAD_REQUEST,
                HAL_CONTENT_TYPE,
                json!({ "error": msg }),
            ),
            Error::Db(e) => {
                tracing::error!(error = %e, "database error");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    JSON_CONTENT_TYPE,
                    json!({ "error": "internal error" }),
                )
            }
        };
        let mut res = (status, axum::Json(body)).into_response();
        res.headers_mut()
            .insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
        res
    }
}

pub type Result<T> = std::result::Result<T, Error>;
