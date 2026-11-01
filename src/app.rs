use crate::{config::Config, db::Db, error::Error, http, http::auth::Credentials};
use axum::{
    Router,
    http::{HeaderName, HeaderValue},
    middleware,
    routing::any,
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
        .route("/environments", any(http::environments::environments))
        .route("/environments/{uuid}", any(http::environments::environment))
        .route("/environments/{uuid}/deployed-versions/currently-deployed", any(http::environments::currently_deployed))
        .route("/environments/{uuid}/released-versions/currently-supported", any(http::environments::currently_supported))
        .route("/pacticipants/{name}/versions/{version}/deployed-versions/environment/{uuid}", any(http::environments::record_deployment))
        .route("/pacticipants/{name}/versions/{version}/released-versions/environment/{uuid}", any(http::environments::record_release))
        .route("/deployed-versions/{uuid}", any(http::environments::deployed_version))
        .route("/released-versions/{uuid}", any(http::environments::released_version))
        .route("/webhooks", any(http::webhooks::collection))
        .route("/webhooks/execute", any(http::webhooks::execute_unsaved))
        .route("/webhooks/provider/{provider}/consumer/{consumer}", any(http::webhooks::consumer_provider_collection))
        .route("/webhooks/provider/{provider}", any(http::webhooks::provider_collection))
        .route("/webhooks/consumer/{consumer}", any(http::webhooks::consumer_collection))
        .route("/webhooks/{uuid}", any(http::webhooks::webhook))
        .route("/webhooks/{uuid}/execute", any(http::webhooks::execute))
        .route("/pacts/provider/{provider}/consumer/{consumer}/webhooks", any(http::webhooks::pact_collection))
        .route("/pacts/provider/{provider}/consumer/{consumer}/webhooks/status", any(http::webhooks::pact_webhooks_status))
        .route("/pacts/provider/{provider}/consumer/{consumer}/version/{version}/triggered-webhooks", any(http::webhooks::pact_triggered_webhooks))
        .route("/pacts/provider/{provider}/consumer/{consumer}/pact-version/{sha}/verification-results/{number}/triggered-webhooks", any(http::webhooks::verification_triggered_webhooks))
        .route("/triggered-webhooks/{uuid}/logs", any(http::webhooks::triggered_webhook_logs))
        .route("/integrations", any(http::pact_lists::integrations))
        .route("/contracts/publish", any(http::contracts::publish))
        .route("/pacts/latest", any(http::pact_lists::latest))
        .route(
            "/pacts/provider/{provider}",
            any(http::pact_lists::provider_all),
        )
        .route(
            "/pacts/provider/{provider}/latest",
            any(http::pact_lists::provider_latest),
        )
        .route(
            "/pacts/provider/{provider}/latest/{tag}",
            any(http::pact_lists::provider_latest_with_tag),
        )
        .route(
            "/pacts/provider/{provider}/tag/{tag}",
            any(http::pact_lists::provider_with_tag),
        )
        .route(
            "/pacts/provider/{provider}/branch",
            any(http::pact_lists::provider_main_branch),
        )
        .route(
            "/pacts/provider/{provider}/branch/latest",
            any(http::pact_lists::provider_latest_main_branch),
        )
        .route(
            "/pacts/provider/{provider}/branch/{branch}",
            any(http::pact_lists::provider_branch),
        )
        .route(
            "/pacts/provider/{provider}/branch/{branch}/latest",
            any(http::pact_lists::provider_latest_branch),
        )
        .route(
            "/pacts/provider/{provider}/consumer/{consumer}/versions",
            any(http::pact_lists::integration_versions),
        )
        .route(
            "/pacts/provider/{provider}/consumer/{consumer}/tag/{tag}",
            any(http::pact_lists::integration_with_tag),
        )
        .route(
            "/pacts/provider/{provider}/consumer/{consumer}/branch/versions",
            any(http::pact_lists::integration_main_branch),
        )
        .route(
            "/pacts/provider/{provider}/consumer/{consumer}/branch/latest/versions",
            any(http::pact_lists::integration_latest_main_branch),
        )
        .route(
            "/pacts/provider/{provider}/consumer/{consumer}/branch/{branch}",
            any(http::pact_lists::integration_branch),
        )
        .route(
            "/pacts/provider/{provider}/consumer/{consumer}/branch/{branch}/latest/versions",
            any(http::pact_lists::integration_latest_branch),
        )
        .route(
            "/pacts/provider/{provider}/consumer/{consumer}/version/{version}",
            any(http::pacts::pact_by_version),
        )
        .route(
            "/pacts/provider/{provider}/consumer/{consumer}/latest",
            any(http::pacts::latest_pact),
        )
        .route(
            "/pacts/provider/{provider}/consumer/{consumer}/latest/{tag}",
            any(http::pacts::latest_pact_with_tag),
        )
        .route(
            "/pacts/provider/{provider}/consumer/{consumer}/latest-untagged",
            any(http::pacts::latest_pact_untagged),
        )
        .route(
            "/pacts/provider/{provider}/consumer/{consumer}/pact-version/{sha}",
            any(http::pacts::pact_by_sha),
        )
        .route("/pacts/provider/{provider}/consumer/{consumer}/pact-version/{sha}/metadata/{metadata}", any(http::verification::pact_with_metadata))
        .route("/pacts/provider/{provider}/consumer/{consumer}/pact-version/{sha}/metadata/{metadata}/verification-results", any(http::verification::results_with_metadata))
        .route("/pacts/provider/{provider}/consumer/{consumer}/pact-version/{sha}/verification-results", any(http::verification::results))
        .route("/pacts/provider/{provider}/consumer/{consumer}/pact-version/{sha}/verification-results/{number}", any(http::verification::result))
        .route("/pacts/provider/{provider}/consumer/{consumer}/version/{version}/verification-results/latest", any(http::verification::latest_result_for_version))
        .route("/pacts/provider/{provider}/for-verification", any(http::verification::for_verification))
        .route("/verification-results/consumer/{consumer}/version/{version}/latest", any(http::verification::latest_results_for_consumer_version))
        .route("/matrix", any(http::matrix::matrix))
        .route("/matrix/provider/{provider}/consumer/{consumer}", any(http::matrix::integration))
        .route("/can-i-deploy", any(http::matrix::can_i_deploy))
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
