use crate::{config::Config, db::Db, error::Error, http, http::auth::Credentials};
use axum::{
    Router,
    http::{HeaderName, HeaderValue},
    middleware,
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
    let creds = Credentials::from_config(&config)?;
    let state = AppState { db, config };
    Ok(Router::new()
        .route("/", any(http::index::index))
        .route("/pacticipants", any(http::pacticipants::collection))
        .route("/pacticipants/{name}", any(http::pacticipants::resource))
        .route(
            "/pacticipants/{name}/versions",
            any(http::versions::versions),
        )
        .route(
            "/pacticipants/{name}/versions/{version}",
            any(http::versions::version),
        )
        .route(
            "/pacticipants/{name}/versions/{version}/tags/{tag}",
            any(http::versions::tag),
        )
        .route(
            "/pacticipants/{name}/branches",
            any(http::versions::branches),
        )
        .route(
            "/pacticipants/{name}/branches/{branch}",
            any(http::versions::branch),
        )
        .route(
            "/pacticipants/{name}/branches/{branch}/versions/{version}",
            any(http::versions::branch_version),
        )
        .route(
            "/pacticipants/{name}/branches/{branch}/latest-version",
            any(http::versions::branch_latest_version),
        )
        .route(
            "/pacticipants/{name}/latest-version",
            any(http::versions::latest_version),
        )
        .route(
            "/pacticipants/{name}/latest-version/{tag}",
            any(http::versions::latest_version_with_tag),
        )
        .route("/environments", get(http::environments::list))
        .route("/webhooks", get(http::webhooks::list))
        .route("/integrations", get(http::integrations::list))
        .route("/pacts/latest", get(http::pacts::latest))
        .route("/matrix", get(http::matrix::matrix))
        .route("/can-i-deploy", get(http::matrix::can_i_deploy))
        .fallback(|| async { Error::NoRoute })
        .layer(middleware::from_fn_with_state(creds, http::auth::require))
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
