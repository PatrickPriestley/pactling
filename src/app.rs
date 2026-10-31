use crate::{config::Config, error::Error, http};
use axum::{
    Router,
    http::{HeaderName, HeaderValue},
    routing::{any, get},
};
use tower_http::set_header::SetResponseHeaderLayer;

pub async fn build(_config: Config) -> anyhow::Result<Router> {
    Ok(Router::new()
        .route("/", any(http::index::index))
        .route(
            "/pacticipants/{name}",
            get(http::pacticipants::get_pacticipant),
        )
        .fallback(|| async { Error::NoRoute })
        .layer(SetResponseHeaderLayer::overriding(
            HeaderName::from_static("x-pact-broker-version"),
            HeaderValue::from_static(env!("CARGO_PKG_VERSION")),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            HeaderName::from_static("x-content-type-options"),
            HeaderValue::from_static("nosniff"),
        )))
}
