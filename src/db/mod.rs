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
use sqlx::{Either, Executor, Sqlite, Transaction, pool::PoolConnection};

/// Where statements run: the pool, or one transaction shared by every clone of a
/// transactional `Db`. Each statement locks the transaction for its duration.
#[derive(Clone)]
pub enum Conn {
    Pool(SqlitePool),
    Tx(Arc<tokio::sync::Mutex<Transaction<'static, Sqlite>>>),
    Read(Arc<tokio::sync::Mutex<ReadConnection>>),
}

impl std::fmt::Debug for Conn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Conn::Pool(_) => f.write_str("Conn::Pool"),
            Conn::Tx(_) => f.write_str("Conn::Tx"),
            Conn::Read(_) => f.write_str("Conn::Read"),
        }
    }
}

/// Owns one pooled connection until a read snapshot is reset. The fail-closed
/// close-on-drop flag is installed before any PRAGMA or BEGIN can be cancelled.
#[derive(Debug)]
pub struct ReadConnection {
    connection: Option<PoolConnection<Sqlite>>,
}

impl ReadConnection {
    fn new(mut connection: PoolConnection<Sqlite>) -> Self {
        connection.close_on_drop();
        Self {
            connection: Some(connection),
        }
    }

    fn connection(&mut self) -> &mut sqlx::SqliteConnection {
        self.connection
            .as_mut()
            .expect("read connection owned until cleanup")
    }

    async fn reset(mut connection: PoolConnection<Sqlite>) -> sqlx::Result<()> {
        // Setup can be cancelled before BEGIN reaches SQLite. This exact response
        // is the one safe rollback error (probed against bundled SQLite 3.51.3).
        if let Err(error) = sqlx::query("ROLLBACK").execute(&mut *connection).await {
            let not_started = error.as_database_error().is_some_and(|error| {
                error.code().as_deref() == Some("1")
                    && error.message() == "cannot rollback - no transaction is active"
            });
            if !not_started {
                return Err(error); // close-on-drop: never pool uncertain state
            }
        }
        sqlx::query("PRAGMA query_only = OFF")
            .execute(&mut *connection)
            .await?;
        // Explicit return is only allowed after both reset operations succeeded.
        // SQLx consumes the live connection here even with close_on_drop set.
        connection.return_to_pool().await;
        Ok(())
    }

    fn cleanup(&mut self) -> tokio::task::JoinHandle<sqlx::Result<()>> {
        let connection = self.connection.take().expect("cleanup runs once");
        tokio::spawn(Self::reset(connection))
    }

    async fn finish(mut self) -> sqlx::Result<()> {
        // Awaiting a detached cleanup task lets cancellation of commit itself
        // leave cleanup running, preserving the single in-memory connection.
        self.cleanup()
            .await
            .map_err(|error| sqlx::Error::Protocol(error.to_string()))?
    }
}

impl Drop for ReadConnection {
    fn drop(&mut self) {
        if self.connection.is_some() {
            // A detached task owns the connection until it is safe to reuse.
            // If the runtime cancels that task, close_on_drop prevents leakage.
            drop(self.cleanup());
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
            Conn::Read(read) => {
                let read = read.clone();
                Box::pin(
                    futures_util::stream::once(async move {
                        let mut guard = read.lock().await;
                        guard
                            .connection()
                            .fetch_many(query)
                            .try_collect::<Vec<_>>()
                            .await
                    })
                    .flat_map(|result| match result {
                        Ok(items) => {
                            futures_util::stream::iter(items.into_iter().map(Ok)).left_stream()
                        }
                        Err(error) => {
                            futures_util::stream::once(async move { Err(error) }).right_stream()
                        }
                    }),
                )
            }
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
            Conn::Read(read) => {
                let read = read.clone();
                Box::pin(async move {
                    let mut guard = read.lock().await;
                    guard.connection().fetch_optional(query).await
                })
            }
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
            Conn::Read(read) => {
                let read = read.clone();
                Box::pin(async move {
                    let mut guard = read.lock().await;
                    guard.connection().prepare_with(sql, parameters).await
                })
            }
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
            Conn::Read(read) => {
                let read = read.clone();
                Box::pin(async move {
                    let mut guard = read.lock().await;
                    guard.connection().describe(sql).await
                })
            }
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
            // SQLx may discard an idle connection if acquisition is cancelled
            // during its health check. A one-connection in-memory database would
            // disappear with it; no acquire/connect hooks add other await points.
            .test_before_acquire(!memory)
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
        assert!(
            !matches!(self.conn, Conn::Read(_)),
            "read snapshot cannot expose its backing pool"
        );
        &self.pool
    }

    /// A `Db` whose every statement runs in one transaction, taken with `BEGIN IMMEDIATE`
    /// so a concurrent writer waits rather than failing later. Dropping it without
    /// `commit` rolls back. Inside a transaction use only the transactional `Db`; on a
    /// `Db` that already is one, this returns a clone sharing the transaction.
    pub async fn begin(&self) -> sqlx::Result<Db> {
        match &self.conn {
            Conn::Tx(_) => Ok(self.clone()),
            Conn::Read(_) => Err(sqlx::Error::Protocol(
                "cannot upgrade a read snapshot to a write transaction".into(),
            )),
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

    /// Open a deferred, read-only snapshot. Its first statement after BEGIN reads
    /// broker data to pin the WAL snapshot before this handle is returned. Existing
    /// transactions cannot be reused: a write transaction would retain its write lock.
    /// All reads must use this Db, not the pool; in-memory pools have one connection.
    /// SQLite query_only rejects writes through the handle. Commit/drop/error/cancel
    /// resets the connection before pooling it; a failed reset closes it instead.
    pub async fn begin_read(&self) -> sqlx::Result<Db> {
        let Conn::Pool(pool) = &self.conn else {
            return Err(sqlx::Error::Protocol(
                "cannot open a read snapshot inside a transaction".into(),
            ));
        };
        let mut read = ReadConnection::new(pool.acquire().await?);
        sqlx::query("PRAGMA query_only = ON")
            .execute(read.connection())
            .await?;
        sqlx::query("BEGIN").execute(read.connection()).await?;
        sqlx::query("SELECT id FROM pacticipants LIMIT 1")
            .fetch_optional(read.connection())
            .await?;
        Ok(Db {
            pool: self.pool.clone(),
            conn: Conn::Read(Arc::new(tokio::sync::Mutex::new(read))),
            owner: true,
        })
    }

    /// Commits the transaction this handle opened. On a handle handed to an inner
    /// operation it does nothing; on the owner it is an error while such a handle is
    /// still alive, since that handle's drop would roll back what was just reported
    /// committed (review finding).
    pub async fn commit(self) -> sqlx::Result<()> {
        match self.conn {
            Conn::Pool(_) => Ok(()),
            Conn::Tx(_) | Conn::Read(_) if !self.owner => Ok(()),
            Conn::Read(read) => match Arc::try_unwrap(read) {
                Ok(mutex) => mutex.into_inner().finish().await,
                Err(_) => Err(sqlx::Error::Protocol(
                    "cannot finish: the read snapshot is still shared with an inner operation"
                        .into(),
                )),
            },
            Conn::Tx(tx) => match Arc::try_unwrap(tx) {
                Ok(mutex) => mutex.into_inner().commit().await,
                Err(_) => Err(sqlx::Error::Protocol(
                    "cannot commit: the transaction is still shared with an inner operation".into(),
                )),
            },
        }
    }

    pub async fn migrate(&self) -> sqlx::Result<()> {
        if matches!(self.conn, Conn::Read(_)) {
            return Err(sqlx::Error::Protocol(
                "cannot migrate through a read snapshot".into(),
            ));
        }
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
