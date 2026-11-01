use clap::Parser;
use pactling::{app, config::Config};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    if std::env::args_os().nth(1).as_deref() == Some(std::ffi::OsStr::new("explain")) {
        std::process::exit(pactling::cli::run(std::env::args_os().skip(1)).await);
    }
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    let config = Config::parse();
    let port = config.port;
    let router = app::build(config).await?;
    let listener = tokio::net::TcpListener::bind(("0.0.0.0", port)).await?;
    tracing::info!("pactling listening on {}", listener.local_addr()?);
    axum::serve(listener, router).await?;
    Ok(())
}
