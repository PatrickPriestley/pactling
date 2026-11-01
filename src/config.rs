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

    /// Schemes a webhook URL may use (Ruby broker default: https only).
    #[arg(
        long,
        env = "PACT_BROKER_WEBHOOK_SCHEME_WHITELIST",
        value_delimiter = ' ',
        default_value = "https"
    )]
    pub webhook_scheme_whitelist: Vec<String>,

    /// Hosts a webhook may call; empty allows any host but hides response details from
    /// the execution logs, as on the Ruby broker.
    #[arg(
        long,
        env = "PACT_BROKER_WEBHOOK_HOST_WHITELIST",
        value_delimiter = ' ',
        default_value = ""
    )]
    pub webhook_host_whitelist: Vec<String>,

    /// HTTP methods a webhook may use (Ruby broker default: POST).
    #[arg(
        long,
        env = "PACT_BROKER_WEBHOOK_HTTP_METHOD_WHITELIST",
        value_delimiter = ' ',
        default_value = "POST"
    )]
    pub webhook_http_method_whitelist: Vec<String>,

    /// Response codes that count as a successful delivery.
    #[arg(long, env = "PACT_BROKER_WEBHOOK_HTTP_CODE_SUCCESS", value_delimiter = ' ', default_values_t = [200, 201, 202, 203, 204, 205, 206])]
    pub webhook_http_code_success: Vec<u16>,

    /// Seconds to wait before each retry of a failed delivery (Ruby broker default:
    /// 10 60 120 300 600 1200, so seven attempts in all).
    #[arg(long, env = "PACT_BROKER_WEBHOOK_RETRY_SCHEDULE", value_delimiter = ' ', default_values_t = [10, 60, 120, 300, 600, 1200])]
    pub webhook_retry_schedule: Vec<u64>,

    /// Seconds before the first delivery attempt of the i-th webhook triggered by one
    /// request: `initial + i * step`, as the Ruby broker schedules them (5 + 3i).
    #[arg(long, env = "PACTLING_WEBHOOK_INITIAL_DELAY", default_value_t = 5)]
    pub webhook_initial_delay: u64,
    #[arg(long, env = "PACTLING_WEBHOOK_DELAY_STEP", default_value_t = 3)]
    pub webhook_delay_step: u64,
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
            webhook_scheme_whitelist: vec!["https".into()],
            webhook_host_whitelist: vec![],
            webhook_http_method_whitelist: vec!["POST".into()],
            webhook_http_code_success: vec![200, 201, 202, 203, 204, 205, 206],
            webhook_retry_schedule: vec![10, 60, 120, 300, 600, 1200],
            webhook_initial_delay: 5,
            webhook_delay_step: 3,
        }
    }
}
