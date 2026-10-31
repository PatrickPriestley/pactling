use crate::{config::Config, db::Db, error::Error, http};
use axum::{
    Router,
    http::{HeaderName, HeaderValue},
    routing::{any, get},
};
use tower_http::set_header::SetResponseHeaderLayer;

#[derive(Clone)]
pub struct AppState {
    pub db: Db,
    pub config: Config,
}

pub async fn build(config: Config) -> anyhow::Result<Router> {
    let db = Db::connect(&config.database_url).await?;
    db.migrate().await?;
    let state = AppState { db, config };
    Ok(Router::new()
        .route("/", any(http::index::index))
        .route(
            "/pacticipants/{name}",
            get(http::pacticipants::get_pacticipant),
        )
        .route("/pacticipants", get(http::pacticipants::list))
        .route("/environments", get(http::environments::list))
        .route("/webhooks", get(http::webhooks::list))
        .route("/integrations", get(http::integrations::list))
        .route("/pacts/latest", get(http::pacts::latest))
        .route("/matrix", get(http::matrix::matrix))
        .route("/can-i-deploy", get(http::matrix::can_i_deploy))
        .fallback(|| async { Error::NoRoute })
        .layer(SetResponseHeaderLayer::overriding(
            HeaderName::from_static("x-pact-broker-version"),
            HeaderValue::from_static(env!("CARGO_PKG_VERSION")),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            HeaderName::from_static("x-content-type-options"),
            HeaderValue::from_static("nosniff"),
        ))
        .with_state(state))
}
