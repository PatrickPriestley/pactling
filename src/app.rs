use crate::{config::Config, http};
use axum::{
    Router,
    http::{HeaderName, HeaderValue},
    routing::any,
};
use tower_http::set_header::SetResponseHeaderLayer;

pub async fn build(_config: Config) -> anyhow::Result<Router> {
    Ok(Router::new()
        .route("/", any(http::index::index))
        .layer(SetResponseHeaderLayer::overriding(
            HeaderName::from_static("x-pact-broker-version"),
            HeaderValue::from_static(env!("CARGO_PKG_VERSION")),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            HeaderName::from_static("x-content-type-options"),
            HeaderValue::from_static("nosniff"),
        )))
}
