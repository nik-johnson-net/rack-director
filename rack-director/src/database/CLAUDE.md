# Database

## Connections

Every request opens its own SQLite connection (`DatabaseConnectionFactory::open`). `connection.rs::configure`
sets a 5s `busy_timeout` and WAL journal mode on each one, so overlapping writes wait instead of failing
with `SQLITE_BUSY` ("database is locked"). WAL creates `db.sqlite-wal` / `db.sqlite-shm` beside the
database file.

## Transactions

Use `Connection::in_transaction(async |conn| ...)` for multi-statement writes that must land together, and
pass `conn` to the normal async store functions. It commits on `Ok` and rolls back on `Err`, so error paths
never rely on the best-effort rollback in `Transaction::drop`. `Connection::transaction()` is the
lower-level primitive it wraps (`Transaction` derefs to `Connection`).

- It always issues `BEGIN IMMEDIATE`, taking the write lock up front so the busy timeout applies. A deferred
  transaction that reads then writes can fail with `SQLITE_BUSY_SNAPSHOT`, which is not retried. If a
  read-only snapshot transaction is ever needed, add a separate method rather than changing this one.
- It takes `&mut self`, so while a transaction is open the borrow checker rejects any other statement on
  that connection — another task, a `join!` branch, a second `Director` — that would otherwise silently
  join the transaction or be rolled back with it. An `Arc<Connection>` can never open one.
