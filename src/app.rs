use crate::config::Config;
use axum::{Router, routing::get};

pub async fn build(_config: Config) -> anyhow::Result<Router> {
    Ok(Router::new().route("/", get(|| async { "" })))
}
