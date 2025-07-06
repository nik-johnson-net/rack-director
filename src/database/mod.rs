use std::path::Path;

use anyhow::Result;
use rusqlite::Connection;

const LATEST_VERSION: i32 = 2;
const MIGRATIONS: [&str; LATEST_VERSION as usize] = [
    include_str!("migrations/1.sql"),
    include_str!("migrations/2.sql"),
];

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

pub fn is_device_known(conn: &Connection, uuid: &str) -> Result<bool> {
    let mut stmt = conn.prepare("SELECT 1 FROM devices WHERE uuid = ?1")?;
    let exists = stmt.exists([uuid])?;
    Ok(exists)
}

pub fn register_device(conn: &Connection, uuid: &str) -> Result<()> {
    conn.execute("INSERT OR IGNORE INTO devices (uuid) VALUES (?1)", [uuid])?;

    conn.execute(
        "UPDATE devices SET last_seen_at = CURRENT_TIMESTAMP WHERE uuid = ?1",
        [uuid],
    )?;

    Ok(())
}

pub fn create_interface(conn: &Connection, device_id: i32, mac_address: &str, is_bmc: bool) -> Result<i32> {
    conn.execute(
        "INSERT INTO interfaces (device_id, mac_address, is_bmc) VALUES (?1, ?2, ?3)",
        [&device_id.to_string(), mac_address, &is_bmc.to_string()]
    )?;
    
    Ok(conn.last_insert_rowid() as i32)
}

pub fn update_interface_ip(conn: &Connection, interface_id: i32, ipv4_address: Option<&str>, ipv6_address: Option<&str>) -> Result<()> {
    conn.execute(
        "UPDATE interfaces SET ipv4_address = ?1, ipv6_address = ?2, updated_at = CURRENT_TIMESTAMP WHERE id = ?3",
        rusqlite::params![ipv4_address, ipv6_address, interface_id]
    )?;
    
    Ok(())
}

pub fn find_interface_by_mac(conn: &Connection, mac_address: &str) -> Result<Option<i32>> {
    let mut stmt = conn.prepare("SELECT id FROM interfaces WHERE mac_address = ?1")?;
    let mut rows = stmt.query_map([mac_address], |row| {
        Ok(row.get::<_, i32>(0)?)
    })?;
    
    if let Some(row) = rows.next() {
        Ok(Some(row?))
    } else {
        Ok(None)
    }
}

pub fn create_subnet(conn: &Connection, name: &str, network_ipv4: Option<&str>, network_ipv6: Option<&str>, 
                    gateway_ipv4: Option<&str>, gateway_ipv6: Option<&str>, dns_servers: &str, lease_time: u32) -> Result<i32> {
    conn.execute(
        "INSERT INTO subnets (name, network_ipv4, network_ipv6, gateway_ipv4, gateway_ipv6, dns_servers, lease_time) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        rusqlite::params![name, network_ipv4, network_ipv6, gateway_ipv4, gateway_ipv6, dns_servers, lease_time]
    )?;
    
    Ok(conn.last_insert_rowid() as i32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn setup_test_db() -> (Connection, tempfile::TempDir) {
        let temp_dir = tempdir().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let conn = open(&db_path).unwrap();
        (conn, temp_dir)
    }

    #[test]
    fn test_device_operations() {
        let (conn, _temp_dir) = setup_test_db();
        let test_uuid = "550e8400-e29b-41d4-a716-446655440000";

        assert!(!is_device_known(&conn, test_uuid).unwrap());

        register_device(&conn, test_uuid).unwrap();

        assert!(is_device_known(&conn, test_uuid).unwrap());

        register_device(&conn, test_uuid).unwrap();
        assert!(is_device_known(&conn, test_uuid).unwrap());
    }

    #[test]
    fn test_multiple_devices() {
        let (conn, _temp_dir) = setup_test_db();
        let uuid1 = "550e8400-e29b-41d4-a716-446655440001";
        let uuid2 = "550e8400-e29b-41d4-a716-446655440002";

        register_device(&conn, uuid1).unwrap();
        assert!(is_device_known(&conn, uuid1).unwrap());
        assert!(!is_device_known(&conn, uuid2).unwrap());

        register_device(&conn, uuid2).unwrap();
        assert!(is_device_known(&conn, uuid1).unwrap());
        assert!(is_device_known(&conn, uuid2).unwrap());
    }
}
