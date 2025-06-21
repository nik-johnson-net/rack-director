use std::path::Path;

use anyhow::Result;
use rusqlite::Connection;

const LATEST_VERSION: i32 = 1;
const MIGRATIONS: [&str; LATEST_VERSION as usize] = [include_str!("migrations/1.sql")];

pub fn open<T: AsRef<Path>>(path: T) -> Result<Connection> {
    let conn = rusqlite::Connection::open(path)?;
    let current_version = get_or_init_current_migration(&conn)?;
    perform_migrations(&conn, current_version)?;
    Ok(conn)
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
        perform_migration(conn, version + 1)?;
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
