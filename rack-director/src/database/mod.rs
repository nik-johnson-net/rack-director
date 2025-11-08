use std::path::Path;

use anyhow::Result;
use rusqlite::Connection;

const LATEST_VERSION: i32 = 5;
const MIGRATIONS: [&str; LATEST_VERSION as usize] = [
    include_str!("migrations/1.sql"),
    include_str!("migrations/2.sql"),
    include_str!("migrations/3.sql"),
    include_str!("migrations/4.sql"),
    include_str!("migrations/5.sql"),
];

pub fn open<T: AsRef<Path>>(path: T) -> Result<Connection> {
    let conn = Connection::open(path)?;
    let current_version = get_or_init_current_migration(&conn)?;
    perform_migrations(&conn, current_version)?;
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
    if let Err(e) = conn.execute_batch(MIGRATIONS[version as usize - 1]) {
        log::error!("Couldn't update database. {e}");
        return Err(e.into());
    }

    conn.execute("UPDATE migrations SET version = ?1 ", [version])?;

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
        let temp_dir = tempdir().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let conn = open(db_path).unwrap();

        // Test creating a device
        conn.execute(
            "INSERT INTO devices (uuid, lifecycle) VALUES (?1, 'new')",
            ["test-uuid"],
        )
        .unwrap();

        // Test querying the device
        let mut stmt = conn
            .prepare("SELECT uuid FROM devices WHERE uuid = ?1")
            .unwrap();
        let uuid: String = stmt.query_row(["test-uuid"], |row| row.get(0)).unwrap();

        assert_eq!(uuid, "test-uuid");
    }
}
