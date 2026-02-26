use std::{
    ops::Deref,
    path::Path,
    thread::{self, JoinHandle},
};

use rusqlite::Params;
use tokio::sync::{mpsc, oneshot};

/// SqlRequest is the function to be called with the rusqlite Connection on the
/// remote thread.
trait SqlRequest: Send {
    fn handle(self: Box<Self>, conn: &mut rusqlite::Connection);
}

/// Function on conn that can be sent to the sqlite thread
type SqlFunction<R> = Box<dyn FnOnce(&mut rusqlite::Connection) -> R + Send + 'static>;

/// RequestWrapper wraps a higher level request to respond over a oneshot channel,
/// so that results may be returned to the originating thread. It implements
/// SqlRequest.
struct RequestWrapper<Response: Send> {
    action: SqlFunction<Response>,
    reply: oneshot::Sender<Response>,
}

impl<Response: Send> RequestWrapper<Response> {
    fn new(action: SqlFunction<Response>, reply: oneshot::Sender<Response>) -> Self {
        Self { action, reply }
    }
}

impl<Response: Send> SqlRequest for RequestWrapper<Response> {
    fn handle(self: Box<Self>, conn: &mut rusqlite::Connection) {
        let response = (self.action)(conn);
        let _ = self.reply.send(response);
    }
}

fn start_with_file<P: AsRef<Path>>(
    path: P,
    started: oneshot::Sender<rusqlite::Result<mpsc::Sender<ControlMessage>>>,
) {
    let conn = match rusqlite::Connection::open(path) {
        Ok(conn) => conn,
        Err(e) => {
            started
                .send(Err(e))
                .expect("caller did not wait for a response on started channel");
            return;
        }
    };

    let (tx, rx) = mpsc::channel(1);
    started
        .send(Ok(tx))
        .expect("caller did not wait for a response on started channel");

    sqlite_loop(conn, rx);
}

/// Macro to get the path to this test's database. Must be called at the top level of the test.
#[cfg(test)]
#[macro_export]
macro_rules! test_database_path {
    () => {
        format!("file:{}?mode=memory&cache=shared", stdext::function_name!())
    };
}

/// Launched in a background thread to respond to SqlRequest objects.
fn sqlite_loop(mut conn: rusqlite::Connection, mut channel: mpsc::Receiver<ControlMessage>) {
    while let Some(msg) = channel.blocking_recv() {
        match msg {
            ControlMessage::Request(sql_request) => sql_request.handle(&mut conn),
            ControlMessage::Close(sender) => {
                channel.close();

                let mut closers = vec![sender];

                // Drain the queue. Note that this _really_ shouldn't happen since the design is guarded on &mut.
                while let Some(msg) = channel.blocking_recv() {
                    match msg {
                        ControlMessage::Request(sql_request) => sql_request.handle(&mut conn),
                        ControlMessage::Close(sender2) => closers.push(sender2),
                    }
                }

                // Notify requester when closed.
                closers.into_iter().for_each(|sender| {
                    let _ = sender.send(());
                });
            }
        }
    }
}
enum ControlMessage {
    Request(Box<dyn SqlRequest>),
    Close(oneshot::Sender<()>),
}

/// A Connection to a sqlite database. Provides a tokio-compatible async interface to
/// rusqlite.
pub struct Connection {
    handle: JoinHandle<()>,
    tx: mpsc::Sender<ControlMessage>,
}

impl Connection {
    /// Open a database
    pub async fn open<P: AsRef<Path> + Send + 'static>(path: P) -> anyhow::Result<Self> {
        let (start_tx, start_rx) = oneshot::channel();
        let handle = thread::spawn(move || start_with_file(path, start_tx));
        let tx = start_rx.await??;

        Ok(Self { handle, tx })
    }

    /// Convenience method to prepare and execute a single SQL statement.
    ///
    /// On success, returns the number of rows that were changed or inserted or deleted.
    pub async fn execute<Sql: AsRef<str> + Send + 'static, P: Params + Send + 'static>(
        &self,
        sql: Sql,
        params: P,
    ) -> rusqlite::Result<usize> {
        let action: SqlFunction<_> = Box::new(move |conn| conn.execute(sql.as_ref(), params));
        self.send_request(action).await
    }

    /// Convenience method to run multiple SQL statements (that cannot take any parameters).
    pub async fn execute_batch<Sql: AsRef<str> + Send + 'static>(
        &self,
        sql: Sql,
    ) -> rusqlite::Result<()> {
        let action: SqlFunction<_> = Box::new(move |conn| conn.execute_batch(sql.as_ref()));
        self.send_request(action).await
    }

    /// Convenience method to execute a query that is expected to return exactly one row.
    ///
    /// Returns Err(QueryReturnedMoreThanOneRow) if the query returns more than one row.
    ///
    /// Returns Err(QueryReturnedNoRows) if no results are returned. If the query truly is optional, you can call .optional() on the result of this to get a Result<Option<T>> (requires that the trait rusqlite::OptionalExtension is imported).
    pub async fn query_one<
        R: Send + 'static,
        Sql: AsRef<str> + Send + 'static,
        P: Params + Send + 'static,
        F: (FnOnce(&rusqlite::Row<'_>) -> rusqlite::Result<R>) + Send + 'static,
    >(
        &self,
        sql: Sql,
        params: P,
        f: F,
    ) -> rusqlite::Result<R> {
        let action: SqlFunction<_> = Box::new(move |conn| conn.query_one(sql.as_ref(), params, f));
        self.send_request(action).await
    }

    /// Convenience method to execute a query that is expected to return a single row.
    pub async fn query_row<
        R: Send + 'static,
        Sql: AsRef<str> + Send + 'static,
        P: Params + Send + 'static,
        F: (FnOnce(&rusqlite::Row<'_>) -> rusqlite::Result<R>) + Send + 'static,
    >(
        &self,
        sql: Sql,
        params: P,
        f: F,
    ) -> rusqlite::Result<R> {
        let action: SqlFunction<_> = Box::new(move |conn| conn.query_row(sql.as_ref(), params, f));
        self.send_request(action).await
    }

    /// Query Rows and return the results as a Vec<R>
    pub async fn query<
        R: Send + 'static,
        Sql: AsRef<str> + Send + 'static,
        P: Params + Send + 'static,
        F: (FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<R>) + Send + 'static,
    >(
        &self,
        sql: Sql,
        params: P,
        f: F,
    ) -> rusqlite::Result<Vec<R>> {
        let action: SqlFunction<_> = Box::new(move |conn| {
            let mut stmt = conn.prepare(sql.as_ref())?;
            stmt.query_map(params, f)?
                .collect::<rusqlite::Result<Vec<R>>>()
        });
        self.send_request(action).await
    }

    /// Check if table_name exists.
    pub async fn table_exists<T: AsRef<str> + Send + 'static>(
        &self,
        table: T,
    ) -> rusqlite::Result<bool> {
        let action: SqlFunction<_> = Box::new(move |conn| conn.table_exists(None, table.as_ref()));
        self.send_request(action).await
    }

    /// Start a new Transaction.
    pub async fn transaction(&mut self) -> rusqlite::Result<Transaction<'_>> {
        Transaction::new(self).await
    }

    /// Returns the rowid of the most recent successful INSERT in this connection.
    pub async fn last_insert_rowid(&self) -> i64 {
        let action: SqlFunction<_> = Box::new(move |conn| conn.last_insert_rowid());
        self.send_request(action).await
    }

    /// Send a request message to the background thread.
    async fn send_request<R: Send + 'static>(&self, function: SqlFunction<R>) -> R {
        let (tx, rx) = oneshot::channel();
        let request = RequestWrapper::new(function, tx);
        self.tx
            .send(ControlMessage::Request(Box::new(request)))
            .await
            .expect("couldn't send Request to background thread");

        rx.await
            .expect("didn't receive result from background thread")
    }

    /// Closes the connection, consuming self.
    #[allow(unused)]
    pub async fn close(self) {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(ControlMessage::Close(tx))
            .await
            .expect("couldn't send Close request to background thread");
        rx.await
            .expect("didn't receive drained notification from background thread");
        self.handle.join().expect("couldn't join background thread");
    }
}

/// Represents a Transaction on the connection. Must be closed with commit() or rollback().
///
/// If dropped without calling `commit()` or `rollback()`, a best-effort ROLLBACK
/// is issued via `try_send` so that the connection remains usable for subsequent
/// operations. The rollback is best-effort: if the channel is full the ROLLBACK
/// may not be sent, but in practice the channel capacity is 1 and the background
/// thread is never blocked between operations.
#[must_use]
pub struct Transaction<'conn> {
    conn: &'conn mut Connection,
    /// Set to true once commit() or rollback() has been called so that the
    /// Drop impl does not issue a redundant ROLLBACK.
    done: bool,
}

impl<'conn> Transaction<'conn> {
    async fn new(conn: &'conn mut Connection) -> rusqlite::Result<Self> {
        conn.execute("BEGIN ", ()).await?;
        Ok(Self { conn, done: false })
    }

    /// Commit the transaction to the database.
    pub async fn commit(mut self) -> rusqlite::Result<()> {
        self.done = true;
        self.conn.execute("COMMIT", ()).await.map(|_| ())
    }

    /// Rollback the transaction.
    pub async fn rollback(mut self) -> rusqlite::Result<()> {
        self.done = true;
        self.conn.execute("ROLLBACK", ()).await.map(|_| ())
    }
}

impl<'conn> Drop for Transaction<'conn> {
    fn drop(&mut self) {
        if !self.done {
            // Issue a best-effort ROLLBACK so the connection is not left inside
            // an open transaction. We use try_send rather than blocking_send
            // because blocking_send must not be called from within a Tokio
            // async context (it would panic).
            let (reply_tx, _reply_rx) = oneshot::channel();
            let request: Box<dyn SqlRequest> = Box::new(RequestWrapper::new(
                Box::new(|conn: &mut rusqlite::Connection| {
                    let _ = conn.execute_batch("ROLLBACK");
                }),
                reply_tx,
            ));
            let _ = self.conn.tx.try_send(ControlMessage::Request(request));
        }
    }
}

impl<'conn> Deref for Transaction<'conn> {
    type Target = Connection;

    fn deref(&self) -> &Self::Target {
        self.conn
    }
}

#[cfg(test)]
mod tests {
    use super::Connection;

    #[tokio::test]
    async fn empty_params_tuple() {
        let conn = Connection::open(test_database_path!()).await.unwrap();
        let n = conn
            .execute("CREATE TABLE foo (x INTEGER)", ())
            .await
            .unwrap();
        assert_eq!(n, 0); // DDL statements affect 0 rows

        let n = conn
            .execute("INSERT INTO foo(x) VALUES (?1)", (1,))
            .await
            .unwrap();
        assert_eq!(n, 1);
    }

    #[tokio::test]
    async fn query() {
        let conn = Connection::open(test_database_path!()).await.unwrap();
        let n = conn
            .execute("CREATE TABLE foo (x INTEGER)", ())
            .await
            .unwrap();
        assert_eq!(n, 0); // DDL statements affect 0 rows

        let n = conn
            .execute("INSERT INTO foo(x) VALUES (?1)", (1,))
            .await
            .unwrap();
        assert_eq!(n, 1);

        let result: Vec<i64> = conn
            .query("SELECT * FROM foo WHERE x = ?1", (1,), |row| row.get(0))
            .await
            .unwrap();
        assert_eq!(result, vec![1])
    }

    #[tokio::test]
    async fn transaction() {
        let mut conn = Connection::open(test_database_path!()).await.unwrap();
        conn.execute("CREATE TABLE foo (x INTEGER)", ())
            .await
            .unwrap();

        let tx = conn.transaction().await.expect("start transaction");
        tx.execute("INSERT INTO foo(x) VALUES (1)", ())
            .await
            .unwrap();
        tx.commit().await.expect("commit transaction");

        conn.execute("INSERT INTO foo(x) VALUES (2)", ())
            .await
            .unwrap();
    }

    /// Verify that dropping a Transaction without calling commit() issues a
    /// ROLLBACK and leaves the connection in a reusable state.
    #[tokio::test]
    async fn transaction_drop_rolls_back() {
        let mut conn = Connection::open(test_database_path!()).await.unwrap();
        conn.execute("CREATE TABLE foo (x INTEGER)", ())
            .await
            .unwrap();

        {
            let tx = conn.transaction().await.unwrap();
            tx.execute("INSERT INTO foo(x) VALUES (1)", ())
                .await
                .unwrap();
            // Drop tx without calling commit() or rollback().
            // The Drop impl must issue a best-effort ROLLBACK.
        }

        // Give the background thread a moment to process the ROLLBACK sent via try_send.
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;

        // The inserted row must not be visible.
        let count: i64 = conn
            .query_one("SELECT COUNT(*) FROM foo", [], |r| r.get(0))
            .await
            .unwrap();
        assert_eq!(count, 0, "implicit rollback must discard the inserted row");

        // The connection must be reusable — starting a new transaction must succeed.
        let tx2 = conn
            .transaction()
            .await
            .expect("connection must be reusable after implicit rollback");
        tx2.commit().await.unwrap();
    }
}
