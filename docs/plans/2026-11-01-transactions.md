# Atomic business operations (issue #4)

Branch `feat/transactions`, 2026-11-01. Goal: a publish batch, a deployment or release
record and a verification publication either land whole or not at all, and two
concurrent requests cannot leave incompatible rows.

## Probed facts

- Every `Db` method runs its statements against the pool (128 call sites in `src/db`), so
  a business operation spanning several methods is several SQLite transactions. A
  conflicting concurrent publish can apply its branch and tags before its pact is refused;
  two concurrent deployments of different versions to the same environment and instance
  can both end up `currently_deployed` (verified by the tests in `tests/transactions.rs`,
  which fail on `main`).
- sqlx 0.9: `Pool::begin_with("BEGIN IMMEDIATE")` returns a `Transaction<'static>` that
  rolls back on drop; `&mut Transaction` and `&Pool` both implement `Executor`, whose
  required methods are `fetch_many`, `fetch_optional` and `prepare_with`.
- SQLite serialises writers; `BEGIN IMMEDIATE` takes the write lock up front so a
  concurrent writer waits (`busy_timeout`) instead of failing on lock upgrade. WAL lets
  readers proceed meanwhile.

## Decisions

- `Db` carries a connection that is either the pool or a shared transaction
  (`Conn`, an `Executor` implementation that locks the transaction per statement).
  `Db::begin()` returns a transactional `Db` with the same API; `commit()` commits.
  Rejected: passing an executor into every method (128 signatures) and a single-connection
  pool (reads would wait on every write, and raw BEGIN/COMMIT cannot pin a pooled
  connection).
- `begin()` on a transactional `Db` returns a clone sharing the transaction and its
  `commit()` is a no-op, so an operation can be composed inside a larger one.
- Transactions wrap: the publish batch (version, branch, tags, every contract, webhook
  triggers), the legacy PUT, verification publication with its triggers, deployment and
  release recording, environment update and webhook upsert. Rendering happens after commit.
- File databases open in WAL mode with a 5 s busy timeout; the in-memory test database
  keeps one connection, so inside a transaction only the transactional `Db` may be used
  (a mistake shows up as an acquire timeout, not a hang).

## Tasks

1. `tests/transactions.rs`: concurrent conflicting publishes leave only the winner's
   metadata; concurrent deployments leave one current record; a dropped transaction
   leaves nothing (red on `main`).
2. `Conn` + `Db::begin/commit` in `src/db/mod.rs`; pool references become `&self.conn`.
3. Transaction boundaries in the handlers and services listed above.
4. Document the ordering and retry semantics in `AGENTS.md`.

## Independent review (Codex) and disposition

All three findings verified and fixed test-first: a file database now asserts
`PRAGMA journal_mode = wal`; the webhook upsert looks its uuid up inside the transaction, so
two concurrent PUTs of a new uuid create once and update once instead of the second
failing on the unique constraint; `commit()` is owned by the handle that opened the
transaction, a handle handed to an inner operation cannot commit, and the owner refuses to
commit while such a handle is alive instead of reporting success before a later drop
rolls everything back.
- Second review: `Clone` now sets `owner: false` so a clone can neither commit nor inherit
  commit authority when the owner drops (test `a_cloned_handle_never_commits`, red before
  the change). The WAL assertion was run on a real file database:
  `cargo test --test transactions file_databases_run_in_wal_mode -- --exact` passes.
