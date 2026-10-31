//! All SQL lives here. Handlers and services call these methods and never write queries.
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

    pub async fn migrate(&self) -> sqlx::Result<()> {
        sqlx::migrate!("./migrations")
            .run(&self.pool)
            .await
            .map_err(Into::into)
    }

    pub async fn list_pacticipant_names(&self) -> sqlx::Result<Vec<String>> {
        let rows: Vec<(String,)> = sqlx::query_as("SELECT name FROM pacticipants ORDER BY name")
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.into_iter().map(|(n,)| n).collect())
    }
}
