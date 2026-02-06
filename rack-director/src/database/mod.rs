mod migrations;

use std::path::Path;

use anyhow::Result;
use rusqlite::{Connection, params};

/// Trait for types that can be constructed from a database row.
/// Provides a standard interface for row-to-struct conversion.
pub trait FromRow: Sized {
    /// Construct an instance of Self from a rusqlite Row.
    ///
    /// This method should use named column access (e.g., `row.get("column_name")`)
    /// rather than positional access for better maintainability and robustness.
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self>;
}

/// Execute a query and map all rows to a Vec<T>.
///
/// This is a convenience function for queries that return multiple rows.
/// Uses the FromRow trait to convert each row to the target type.
pub fn query_map_all<T: FromRow>(
    conn: &Connection,
    sql: &str,
    params: &[&dyn rusqlite::types::ToSql],
) -> rusqlite::Result<Vec<T>> {
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(params, |row| T::from_row(row))?;
    rows.collect()
}

/// Execute a query and return the first row as T.
///
/// Returns an error if no rows are found (rusqlite::Error::QueryReturnedNoRows).
pub fn query_one<T: FromRow>(
    conn: &Connection,
    sql: &str,
    params: &[&dyn rusqlite::types::ToSql],
) -> rusqlite::Result<T> {
    conn.query_row(sql, params, |row| T::from_row(row))
}

/// Execute a query and return the first row as Option<T>.
///
/// Returns Ok(None) if no rows are found, Ok(Some(T)) if a row exists.
pub fn query_optional<T: FromRow>(
    conn: &Connection,
    sql: &str,
    params: &[&dyn rusqlite::types::ToSql],
) -> rusqlite::Result<Option<T>> {
    use rusqlite::OptionalExtension;
    conn.query_row(sql, params, |row| T::from_row(row))
        .optional()
}

const LATEST_VERSION: i32 = 12;
const MIGRATIONS: [&str; LATEST_VERSION as usize] = [
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
];

/// Post-migration hooks that run Rust code after SQL migrations
/// Index corresponds to migration version (1-indexed, so POST_MIGRATION_HOOKS[10] runs after migration 11)
type PostMigrationHook = fn(&Connection) -> Result<()>;
const POST_MIGRATION_HOOKS: [Option<PostMigrationHook>; LATEST_VERSION as usize] = [
    None,                                          // Migration 1
    None,                                          // Migration 2
    None,                                          // Migration 3
    None,                                          // Migration 4
    None,                                          // Migration 5
    None,                                          // Migration 6
    None,                                          // Migration 7
    None,                                          // Migration 8
    None,                                          // Migration 9
    None,                                          // Migration 10
    Some(migrations::migration_11::convert_uuids), // Migration 11
    None,                                          // Migration 12
];

pub fn open<T: AsRef<Path>>(path: T) -> Result<Connection> {
    let conn = Connection::open(path)?;
    let current_version = get_or_init_current_migration(&conn)?;
    perform_migrations(&conn, current_version)?;

    // Hack for bad data from Bug #1
    // Note: After migration 11, UUIDs are BLOBs
    // The migration should have already removed this bad data, but we keep this as a safeguard
    conn.execute(
        "DELETE FROM devices WHERE uuid = x'7b757569647d'",
        params![],
    )
    .unwrap();

    Ok(conn)
}

#[cfg(test)]
pub fn run_migrations(conn: &Connection) -> Result<()> {
    let current_version = get_or_init_current_migration(conn)?;
    perform_migrations(conn, current_version)?;
    Ok(())
}

fn get_or_init_current_migration(conn: &Connection) -> Result<i32> {
    log::debug!("Checking for migrations");

    if conn.table_exists(None, "migrations")? {
        let version = conn.query_one("SELECT version FROM migrations", [], |r| r.get(0))?;
        Ok(version)
    } else {
        conn.execute_batch(
            "CREATE TABLE migrations (version INTEGER);
                  INSERT INTO migrations (version) VALUES (0)",
        )?;
        Ok(0)
    }
}

fn perform_migrations(conn: &Connection, current_version: i32) -> Result<()> {
    let mut version = current_version;
    while version < LATEST_VERSION {
        version += 1;
        perform_migration(conn, version)?;
    }

    Ok(())
}

fn perform_migration(conn: &Connection, version: i32) -> Result<()> {
    // Wrap entire migration in a transaction for atomicity
    let tx = conn.unchecked_transaction()?;

    // Run SQL migration
    if let Err(e) = tx.execute_batch(MIGRATIONS[version as usize - 1]) {
        log::error!("Couldn't update database. {e}");
        return Err(e.into());
    }

    // Run post-migration hook if it exists
    if let Some(hook) = POST_MIGRATION_HOOKS[version as usize - 1] {
        log::debug!("Running post-migration hook for version {}", version);
        hook(&tx)?;
    }

    tx.execute("UPDATE migrations SET version = ?1 ", [version])?;

    // Commit the transaction
    tx.commit()?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_open() {
        let temp_dir = tempdir().unwrap();
        let db_path = temp_dir.path().join("test.db");
        assert!(open(db_path).is_ok());
    }

    #[test]
    fn test_database_schema() {
        let temp_dir = tempdir().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let conn = open(db_path).unwrap();

        // Test that tables exist
        let mut stmt = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table'")
            .unwrap();
        let table_names: Vec<String> = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        assert!(table_names.contains(&"devices".to_string()));
        assert!(table_names.contains(&"plans".to_string()));
        assert!(table_names.contains(&"lifecycle_transitions".to_string()));
        assert!(table_names.contains(&"migrations".to_string()));
    }

    #[test]
    fn test_device_operations() {
        use uuid::Uuid;

        let temp_dir = tempdir().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let conn = open(db_path).unwrap();

        // Test creating a device with BLOB UUID
        let test_uuid = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();

        conn.execute(
            "INSERT INTO devices (uuid, lifecycle) VALUES (?1, 'new')",
            params![test_uuid],
        )
        .unwrap();

        // Test querying the device
        let mut stmt = conn
            .prepare("SELECT uuid FROM devices WHERE uuid = ?1")
            .unwrap();
        let retrieved_uuid: Uuid = stmt
            .query_row(params![test_uuid], |row| row.get(0))
            .unwrap();

        assert_eq!(retrieved_uuid, test_uuid);
    }

    #[test]
    fn test_migration_11_uuid_conversion() {
        use uuid::Uuid;

        let temp_dir = tempdir().unwrap();
        let db_path = temp_dir.path().join("test_migration.db");
        let conn = Connection::open(&db_path).unwrap();

        // Set up database to migration 10 (before UUID BLOB migration)
        conn.execute_batch(
            "CREATE TABLE migrations (version INTEGER);
             INSERT INTO migrations (version) VALUES (0)",
        )
        .unwrap();

        // Run migrations 1-10
        for version in 1..=10 {
            conn.execute_batch(MIGRATIONS[version - 1]).unwrap();
            conn.execute("UPDATE migrations SET version = ?1", [version])
                .unwrap();
        }

        // Insert test data with TEXT UUIDs
        let test_uuid1 = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440001").unwrap();
        let test_uuid2 = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440002").unwrap();

        conn.execute(
            "INSERT INTO devices (uuid, lifecycle, architecture) VALUES (?1, 'new', 'x86-64')",
            params![test_uuid1.to_string()],
        )
        .unwrap();

        conn.execute(
            "INSERT INTO devices (uuid, lifecycle, architecture) VALUES (?1, 'new', 'x86-64')",
            params![test_uuid2.to_string()],
        )
        .unwrap();

        // Insert plan with TEXT device_uuid
        conn.execute(
            "INSERT INTO plans (device_uuid, status, total_steps, actions) VALUES (?1, 'pending', 1, '[]')",
            params![test_uuid1.to_string()],
        )
        .unwrap();

        // Verify UUIDs are stored as TEXT before migration
        let uuid_type: String = conn
            .query_row(
                "SELECT typeof(uuid) FROM devices WHERE uuid = ?1",
                params![test_uuid1.to_string()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(uuid_type, "text");

        // Run migration 11
        drop(conn);
        let conn = open(&db_path).unwrap();

        // Verify UUIDs are now stored as BLOB
        let uuid_type: String = conn
            .query_row("SELECT typeof(uuid) FROM devices LIMIT 1", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(uuid_type, "blob");

        // Verify we can read UUIDs as Uuid type
        let mut stmt = conn
            .prepare("SELECT uuid FROM devices ORDER BY uuid")
            .unwrap();
        let uuids: Vec<Uuid> = stmt
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        assert_eq!(uuids.len(), 2);
        assert!(uuids.contains(&test_uuid1));
        assert!(uuids.contains(&test_uuid2));

        // Verify plan device_uuid was also converted
        let plan_uuid: Uuid = conn
            .query_row("SELECT device_uuid FROM plans", [], |row| row.get(0))
            .unwrap();
        assert_eq!(plan_uuid, test_uuid1);
    }
}
