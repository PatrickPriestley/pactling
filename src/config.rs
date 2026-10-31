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
}

impl Config {
    pub fn in_memory() -> Self {
        Self {
            port: 0,
            database_url: "sqlite::memory:".into(),
        }
    }
}
