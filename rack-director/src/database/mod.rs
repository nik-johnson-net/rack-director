mod connection;
mod migrations;

use std::path::Path;

use anyhow::Result;
pub use connection::Connection;

/// Trait for types that can be constructed from a database row.
/// Provides a standard interface for row-to-struct conversion.
pub trait FromRow: Sized {
    /// Construct an instance of Self from a rusqlite Row.
    ///
    /// This method should use named column access (e.g., `row.get("column_name")`)
    /// rather than positional access for better maintainability and robustness.
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self>;
}

const LATEST_VERSION: usize = 13;
const MIGRATIONS: [&str; LATEST_VERSION] = [
    include_str!("migrations/1.sql"),
    include_str!("migrations/2.sql"),
    include_str!("migrations/3.sql"),
    include_str!("migrations/4.sql"),
    include_str!("migrations/5.sql"),
    include_str!("migrations/6.sql"),
    include_str!("migrations/7.sql"),
    include_str!("migrations/8.sql"),
    include_str!("migrations/9.sql"),
    include_str!("migrations/10.sql"),
    include_str!("migrations/11.sql"),
    include_str!("migrations/12.sql"),
    include_str!("migrations/13.sql"),
];

use futures::{FutureExt, future::BoxFuture};

/// Post-migration hooks that run Rust code after SQL migrations
/// Index corresponds to migration version (1-indexed, so POST_MIGRATION_HOOKS[10] runs after migration 11)
type PostMigrationHook = fn(&Connection) -> BoxFuture<Result<()>>;
const POST_MIGRATION_HOOKS: [Option<PostMigrationHook>; LATEST_VERSION] = [
    None,                                                               // Migration 1
    None,                                                               // Migration 2
    None,                                                               // Migration 3
    None,                                                               // Migration 4
    None,                                                               // Migration 5
    None,                                                               // Migration 6
    None,                                                               // Migration 7
    None,                                                               // Migration 8
    None,                                                               // Migration 9
    None,                                                               // Migration 10
    Some(|conn| migrations::migration_11::convert_uuids(conn).boxed()), // Migration 11
    None,                                                               // Migration 12
    None,                                                               // Migration 13
];

pub async fn open<T: AsRef<Path> + Send + 'static>(path: T) -> Result<Connection> {
    let mut conn = Connection::open(path).await?;
    let current_version = get_or_init_current_migration(&mut conn).await?;
    perform_migrations(&mut conn, current_version).await?;

    // Hack for bad data from Bug #1
    // Note: After migration 11, UUIDs are BLOBs
    // The migration should have already removed this bad data, but we keep this as a safeguard
    conn.execute("DELETE FROM devices WHERE uuid = x'7b757569647d'", ())
        .await
        .unwrap();

    Ok(conn)
}

async fn get_or_init_current_migration(conn: &mut Connection) -> Result<usize> {
    log::debug!("Checking for migrations");

    if conn.table_exists("migrations").await? {
        let version = conn
            .query_one("SELECT version FROM migrations", [], |r| r.get(0))
            .await?;
        Ok(version)
    } else {
        conn.execute_batch(
            "CREATE TABLE migrations (version INTEGER);
                  INSERT INTO migrations (version) VALUES (0)",
        )
        .await?;
        Ok(0)
    }
}

async fn perform_migrations(conn: &mut Connection, current_version: usize) -> Result<()> {
    let mut version = current_version;
    while version < LATEST_VERSION {
        version += 1;

        let tx = conn.transaction().await?;

        if let Err(e) = perform_migration(&tx, version).await {
            tx.rollback().await?;
            return Err(e);
        }

        tx.commit().await?;
    }

    Ok(())
}

async fn perform_migration(conn: &Connection, version: usize) -> Result<()> {
    // Run SQL migration
    if let Err(e) = conn.execute_batch(MIGRATIONS[version - 1]).await {
        log::error!("Couldn't update database. {e}");
        return Err(e.into());
    }

    // Run post-migration hook if it exists
    if let Some(ref hook) = POST_MIGRATION_HOOKS[version - 1] {
        log::debug!("Running post-migration hook for version {}", version);
        hook(conn).await?;
    }

    conn.execute("UPDATE migrations SET version = ?1 ", [version])
        .await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::test_database_path;

    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_open() {
        let temp_dir = tempdir().unwrap();
        let db_path = temp_dir.path().join("test.db");
        assert!(open(db_path).await.is_ok());
    }

    #[tokio::test]
    async fn test_database_schema() {
        let conn = open(test_database_path!()).await.unwrap();

        // Test that tables exist
        let table_names: Vec<String> = conn
            .query(
                "SELECT name FROM sqlite_master WHERE type='table'",
                (),
                |row| row.get(0),
            )
            .await
            .unwrap();

        assert!(table_names.contains(&"devices".to_string()));
        assert!(table_names.contains(&"plans".to_string()));
        assert!(table_names.contains(&"lifecycle_transitions".to_string()));
        assert!(table_names.contains(&"migrations".to_string()));
    }

    #[tokio::test]
    async fn test_device_operations() {
        use uuid::Uuid;

        let conn = open(test_database_path!()).await.unwrap();

        // Test creating a device with BLOB UUID
        let test_uuid = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();

        conn.execute(
            "INSERT INTO devices (uuid, lifecycle) VALUES (?1, 'new')",
            (test_uuid,),
        )
        .await
        .unwrap();

        // Test querying the device
        let retrieved_uuid: Uuid = conn
            .query_one(
                "SELECT uuid FROM devices WHERE uuid = ?1",
                (test_uuid,),
                |row| row.get(0),
            )
            .await
            .unwrap();

        assert_eq!(retrieved_uuid, test_uuid);
    }

    #[tokio::test]
    async fn test_migration_11_uuid_conversion() {
        use uuid::Uuid;

        let conn = Connection::open(test_database_path!()).await.unwrap();

        // Set up database to migration 10 (before UUID BLOB migration)
        conn.execute_batch(
            "CREATE TABLE migrations (version INTEGER);
             INSERT INTO migrations (version) VALUES (0)",
        )
        .await
        .unwrap();

        // Run migrations 1-10
        for version in 1..=10 {
            conn.execute_batch(MIGRATIONS[version - 1]).await.unwrap();
            conn.execute("UPDATE migrations SET version = ?1", [version])
                .await
                .unwrap();
        }

        // Insert test data with TEXT UUIDs
        let test_uuid1 = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440001").unwrap();
        let test_uuid2 = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440002").unwrap();

        conn.execute(
            "INSERT INTO devices (uuid, lifecycle, architecture) VALUES (?1, 'new', 'x86-64')",
            (test_uuid1.to_string(),),
        )
        .await
        .unwrap();

        conn.execute(
            "INSERT INTO devices (uuid, lifecycle, architecture) VALUES (?1, 'new', 'x86-64')",
            (test_uuid2.to_string(),),
        )
        .await
        .unwrap();

        // Insert plan with TEXT device_uuid
        conn.execute(
            "INSERT INTO plans (device_uuid, status, total_steps, actions) VALUES (?1, 'pending', 1, '[]')",
            (test_uuid1.to_string(),),
        ).await
        .unwrap();

        // Verify UUIDs are stored as TEXT before migration
        let uuid_type: String = conn
            .query_row(
                "SELECT typeof(uuid) FROM devices WHERE uuid = ?1",
                (test_uuid1.to_string(),),
                |row| row.get(0),
            )
            .await
            .unwrap();
        assert_eq!(uuid_type, "text");

        // Run migration 11
        let conn2 = open(test_database_path!()).await.unwrap();

        // Verify UUIDs are now stored as BLOB
        let uuid_type: String = conn2
            .query_row("SELECT typeof(uuid) FROM devices LIMIT 1", [], |row| {
                row.get(0)
            })
            .await
            .unwrap();
        assert_eq!(uuid_type, "blob");

        // Verify we can read UUIDs as Uuid type
        let uuids: Vec<Uuid> = conn2
            .query("SELECT uuid FROM devices ORDER BY uuid", (), |row| {
                row.get(0)
            })
            .await
            .unwrap();

        assert_eq!(uuids.len(), 2);
        assert!(uuids.contains(&test_uuid1));
        assert!(uuids.contains(&test_uuid2));

        // Verify plan device_uuid was also converted
        let plan_uuid: Uuid = conn2
            .query_row("SELECT device_uuid FROM plans", [], |row| row.get(0))
            .await
            .unwrap();
        assert_eq!(plan_uuid, test_uuid1);
    }
}
