//! All SQL lives here. Handlers and services call these methods and never write queries.
pub mod environments;
pub mod matrix;
pub mod pact_lists;
pub mod pacticipants;
pub mod pacts;
pub mod verifications;
pub mod versions;
pub mod webhooks;

use std::str::FromStr;
use std::sync::Arc;

use futures_core::future::BoxFuture;
use futures_core::stream::BoxStream;
use futures_util::{StreamExt, TryStreamExt};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePool, SqlitePoolOptions};
use sqlx::{Either, Executor, Sqlite, Transaction};

/// Where statements run: the pool, or one transaction shared by every clone of a
/// transactional `Db`. Each statement locks the transaction for its duration.
#[derive(Clone)]
pub enum Conn {
    Pool(SqlitePool),
    Tx(Arc<tokio::sync::Mutex<Transaction<'static, Sqlite>>>),
}

impl std::fmt::Debug for Conn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Conn::Pool(_) => f.write_str("Conn::Pool"),
            Conn::Tx(_) => f.write_str("Conn::Tx"),
        }
    }
}

impl<'c> Executor<'c> for &'c Conn {
    type Database = Sqlite;

    fn fetch_many<'e, 'q: 'e, E>(
        self,
        query: E,
    ) -> BoxStream<
        'e,
        Result<Either<sqlx::sqlite::SqliteQueryResult, sqlx::sqlite::SqliteRow>, sqlx::Error>,
    >
    where
        'c: 'e,
        E: 'q + sqlx::Execute<'q, Sqlite>,
    {
        match self {
            Conn::Pool(pool) => pool.fetch_many(query),
            Conn::Tx(tx) => {
                let tx = tx.clone();
                // The rows are collected while the lock is held, then streamed.
                Box::pin(
                    futures_util::stream::once(async move {
                        let mut guard = tx.lock().await;
                        (&mut **guard)
                            .fetch_many(query)
                            .try_collect::<Vec<_>>()
                            .await
                    })
                    .flat_map(|result| match result {
                        Ok(items) => {
                            futures_util::stream::iter(items.into_iter().map(Ok)).left_stream()
                        }
                        Err(e) => futures_util::stream::once(async move { Err(e) }).right_stream(),
                    }),
                )
            }
        }
    }

    fn fetch_optional<'e, 'q: 'e, E>(
        self,
        query: E,
    ) -> BoxFuture<'e, Result<Option<sqlx::sqlite::SqliteRow>, sqlx::Error>>
    where
        'c: 'e,
        E: 'q + sqlx::Execute<'q, Sqlite>,
    {
        match self {
            Conn::Pool(pool) => pool.fetch_optional(query),
            Conn::Tx(tx) => {
                let tx = tx.clone();
                Box::pin(async move {
                    let mut guard = tx.lock().await;
                    (&mut **guard).fetch_optional(query).await
                })
            }
        }
    }

    fn prepare_with<'e>(
        self,
        sql: sqlx::SqlStr,
        parameters: &'e [sqlx::sqlite::SqliteTypeInfo],
    ) -> BoxFuture<'e, Result<sqlx::sqlite::SqliteStatement, sqlx::Error>>
    where
        'c: 'e,
    {
        match self {
            Conn::Pool(pool) => pool.prepare_with(sql, parameters),
            Conn::Tx(tx) => {
                let tx = tx.clone();
                Box::pin(async move {
                    let mut guard = tx.lock().await;
                    (&mut **guard).prepare_with(sql, parameters).await
                })
            }
        }
    }

    fn describe<'e>(
        self,
        sql: sqlx::SqlStr,
    ) -> BoxFuture<'e, Result<sqlx::Describe<Sqlite>, sqlx::Error>>
    where
        'c: 'e,
    {
        match self {
            Conn::Pool(pool) => pool.describe(sql),
            Conn::Tx(tx) => {
                let tx = tx.clone();
                Box::pin(async move {
                    let mut guard = tx.lock().await;
                    (&mut **guard).describe(sql).await
                })
            }
        }
    }
}

#[derive(Debug)]
pub struct Db {
    pool: SqlitePool,
    conn: Conn,
    /// True for the handle that opened the transaction; a handle handed to an inner
    /// operation cannot commit, and the owner refuses to while such a handle is alive.
    owner: bool,
}

/// A clone shares the pool or the transaction but never the authority to commit it.
impl Clone for Db {
    fn clone(&self) -> Self {
        Db {
            pool: self.pool.clone(),
            conn: self.conn.clone(),
            owner: false,
        }
    }
}

impl Db {
    pub async fn connect(url: &str) -> sqlx::Result<Self> {
        let memory = url.contains(":memory:");
        let mut options = SqliteConnectOptions::from_str(url)?
            // Writers wait for each other instead of failing with "database is locked".
            .busy_timeout(std::time::Duration::from_secs(5));
        if !memory {
            // Readers are not blocked by a writer's transaction.
            options = options.journal_mode(SqliteJournalMode::Wal);
        }
        let pool = SqlitePoolOptions::new()
            // One connection keeps an in-memory database alive and serialises SQLite writes.
            .max_connections(if memory { 1 } else { 4 })
            .acquire_timeout(std::time::Duration::from_secs(10))
            .connect_with(options)
            .await?;
        sqlx::query("PRAGMA foreign_keys = ON")
            .execute(&pool)
            .await?;
        Ok(Self {
            conn: Conn::Pool(pool.clone()),
            pool,
            owner: false,
        })
    }

    /// The raw pool, for tests and migrations only; application code goes through the
    /// methods on `Db`.
    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    /// A `Db` whose every statement runs in one transaction, taken with `BEGIN IMMEDIATE`
    /// so a concurrent writer waits rather than failing later. Dropping it without
    /// `commit` rolls back. Inside a transaction use only the transactional `Db`; on a
    /// `Db` that already is one, this returns a clone sharing the transaction.
    pub async fn begin(&self) -> sqlx::Result<Db> {
        match &self.conn {
            Conn::Tx(_) => Ok(self.clone()),
            Conn::Pool(pool) => {
                let tx = pool.begin_with("BEGIN IMMEDIATE").await?;
                Ok(Db {
                    pool: self.pool.clone(),
                    conn: Conn::Tx(Arc::new(tokio::sync::Mutex::new(tx))),
                    owner: true,
                })
            }
        }
    }

    /// Read-snapshot seam; characterization placeholder uses the old transaction.
    pub async fn begin_read(&self) -> sqlx::Result<Db> {
        self.begin().await
    }

    /// Commits the transaction this handle opened. On a handle handed to an inner
    /// operation it does nothing; on the owner it is an error while such a handle is
    /// still alive, since that handle's drop would roll back what was just reported
    /// committed (review finding).
    pub async fn commit(self) -> sqlx::Result<()> {
        match self.conn {
            Conn::Pool(_) => Ok(()),
            Conn::Tx(_) if !self.owner => Ok(()),
            Conn::Tx(tx) => match Arc::try_unwrap(tx) {
                Ok(mutex) => mutex.into_inner().commit().await,
                Err(_) => Err(sqlx::Error::Protocol(
                    "cannot commit: the transaction is still shared with an inner operation".into(),
                )),
            },
        }
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
