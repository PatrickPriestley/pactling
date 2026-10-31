//! All SQL lives here. Handlers and services call these methods and never write queries.
pub mod pacticipants;
pub mod versions;

use sqlx::sqlite::{SqlitePool, SqlitePoolOptions};

#[derive(Clone)]
pub struct Db {
    pool: SqlitePool,
}

impl Db {
    pub async fn connect(url: &str) -> sqlx::Result<Self> {
        let pool = SqlitePoolOptions::new()
            // One connection keeps an in-memory database alive and serialises SQLite writes.
            .max_connections(if url.contains(":memory:") { 1 } else { 4 })
            .connect(url)
            .await?;
        sqlx::query("PRAGMA foreign_keys = ON")
            .execute(&pool)
            .await?;
        Ok(Self { pool })
    }

    /// The raw pool, for tests and migrations only; application code goes through the
    /// methods on `Db`.
    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    pub async fn migrate(&self) -> sqlx::Result<()> {
        sqlx::migrate!("./migrations")
            .run(&self.pool)
            .await
            .map_err(Into::into)
    }

    pub async fn list_pacticipant_names(&self) -> sqlx::Result<Vec<String>> {
        Ok(self
            .list_pacticipants()
            .await?
            .into_iter()
            .map(|p| p.name)
            .collect())
    }
}

/// Timestamps in the Ruby broker's format: `2026-10-31T02:53:09+00:00`.
pub fn now() -> String {
    chrono::Utc::now()
        .format("%Y-%m-%dT%H:%M:%S+00:00")
        .to_string()
}
