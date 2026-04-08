//! Pre-migration hook for migration 19.
//!
//! Migration 19 drops the `roles` table unconditionally. This hook runs before
//! the SQL and logs a warning if any rows exist, making data loss visible in
//! logs rather than silent.

use crate::database::Connection;
use anyhow::Result;

/// Log a warning listing any existing role names before migration 19 drops the table.
///
/// No-op on fresh databases where the roles table is empty or absent.
pub async fn warn_if_roles_exist(conn: &Connection) -> Result<()> {
    if !conn.table_exists("roles").await? {
        return Ok(());
    }

    let rows: Vec<(i64, String)> = conn
        .query("SELECT id, name FROM roles", (), |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .await?;

    if rows.is_empty() {
        return Ok(());
    }

    log::warn!(
        "Migration 19: dropping {} role(s): {}",
        rows.len(),
        rows.iter()
            .map(|(id, name)| format!("{} (id={})", name, id))
            .collect::<Vec<_>>()
            .join(", ")
    );

    Ok(())
}
