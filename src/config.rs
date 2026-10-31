use clap::Parser;

#[derive(Parser, Debug, Clone)]
#[command(name = "pactling", version, about = "A Pact broker in one binary")]
pub struct Config {
    /// Port to listen on
    #[arg(long, env = "PACT_BROKER_PORT", default_value_t = 9292)]
    pub port: u16,

    /// SQLite database URL. Use `sqlite::memory:` for an in-memory database.
    #[arg(
        long,
        env = "PACTLING_DATABASE_URL",
        default_value = "sqlite://pactling.db?mode=rwc"
    )]
    pub database_url: String,

    /// Read/write basic auth user. Empty means unset, as on the Ruby broker.
    #[arg(long, env = "PACT_BROKER_BASIC_AUTH_USERNAME")]
    pub basic_auth_username: Option<String>,
    #[arg(long, env = "PACT_BROKER_BASIC_AUTH_PASSWORD")]
    pub basic_auth_password: Option<String>,

    /// Read-only basic auth user.
    #[arg(long, env = "PACT_BROKER_BASIC_AUTH_READ_ONLY_USERNAME")]
    pub basic_auth_read_only_username: Option<String>,
    #[arg(long, env = "PACT_BROKER_BASIC_AUTH_READ_ONLY_PASSWORD")]
    pub basic_auth_read_only_password: Option<String>,

    /// Allow unauthenticated reads when basic auth is configured.
    #[arg(
        long,
        env = "PACT_BROKER_ALLOW_PUBLIC_READ",
        default_value_t = false,
        action = clap::ArgAction::Set,
        value_parser = clap::builder::BoolishValueParser::new()
    )]
    pub allow_public_read: bool,

    /// A static bearer token with read/write rights. A pactling extension; the Ruby broker
    /// has no bearer auth.
    #[arg(long, env = "PACTLING_BEARER_TOKEN")]
    pub bearer_token: Option<String>,
}

impl Config {
    pub fn in_memory() -> Self {
        Self {
            port: 0,
            database_url: "sqlite::memory:".into(),
            basic_auth_username: None,
            basic_auth_password: None,
            basic_auth_read_only_username: None,
            basic_auth_read_only_password: None,
            allow_public_read: false,
            bearer_token: None,
        }
    }
}
