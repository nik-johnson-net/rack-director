//! Database access for device warnings.

use anyhow::Result;
use chrono::{DateTime, Utc};
use rusqlite::OptionalExtension;
use serde::Serialize;

use crate::database::{Connection, FromRow};

/// A non-fatal warning attached to a specific device.
///
/// Warnings are created automatically (e.g. when a stale disk label override is removed)
/// and can be dismissed by an operator via the API.
#[derive(Debug, Clone, Serialize)]
pub struct DeviceWarning {
    /// Auto-incremented primary key.
    pub id: i64,
    /// The rowid of the device this warning belongs to.
    pub device_id: i64,
    /// Short machine-readable code identifying the warning type (e.g. `"LABEL_OVERRIDE_DROPPED"`).
    pub code: String,
    /// Human-readable description of the warning.
    pub message: String,
    /// When the warning was created (UTC).
    pub created_at: DateTime<Utc>,
}

impl FromRow for DeviceWarning {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        let created_at_str: String = row.get("created_at")?;
        let created_at = created_at_str
            .parse::<DateTime<Utc>>()
            .unwrap_or_else(|_| Utc::now());
        Ok(DeviceWarning {
            id: row.get("id")?,
            device_id: row.get("device_id")?,
            code: row.get("code")?,
            message: row.get("message")?,
            created_at,
        })
    }
}

/// Create a new warning for a device identified by its integer row `device_id`.
///
/// Returns the newly created warning including its auto-assigned `id` and `created_at`.
pub async fn create_warning(
    conn: &Connection,
    device_id: i64,
    code: &str,
    message: &str,
) -> Result<DeviceWarning> {
    conn.execute(
        "INSERT INTO device_warnings (device_id, code, message) VALUES (?1, ?2, ?3)",
        (device_id, code.to_string(), message.to_string()),
    )
    .await?;

    let warning = conn
        .query_one(
            "SELECT id, device_id, code, message, created_at \
             FROM device_warnings \
             WHERE id = last_insert_rowid()",
            (),
            DeviceWarning::from_row,
        )
        .await?;

    Ok(warning)
}

/// List all warnings for a device identified by its integer row `device_id`.
///
/// Results are ordered by `created_at` ascending so that the oldest warning appears first.
pub async fn list_warnings(conn: &Connection, device_id: i64) -> Result<Vec<DeviceWarning>> {
    let warnings = conn
        .query(
            "SELECT id, device_id, code, message, created_at \
             FROM device_warnings \
             WHERE device_id = ?1 \
             ORDER BY created_at ASC",
            (device_id,),
            DeviceWarning::from_row,
        )
        .await?;

    Ok(warnings)
}

/// Delete a single warning by its `warning_id`, scoped to `device_id` for safety.
///
/// Returns `true` if a row was deleted, `false` if no matching row was found.
pub async fn delete_warning(conn: &Connection, warning_id: i64, device_id: i64) -> Result<bool> {
    let rows_affected = conn
        .execute(
            "DELETE FROM device_warnings WHERE id = ?1 AND device_id = ?2",
            (warning_id, device_id),
        )
        .await?;

    Ok(rows_affected > 0)
}

/// Look up the integer `id` of a device by its UUID string representation.
///
/// Returns `None` when no device with that UUID exists.
pub async fn get_device_id_by_uuid(conn: &Connection, uuid: &uuid::Uuid) -> Result<Option<i64>> {
    let id = conn
        .query_row("SELECT id FROM devices WHERE uuid = ?1", (*uuid,), |row| {
            row.get(0)
        })
        .await
        .optional()?;

    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    use crate::{database, test_connection_factory};

    async fn setup(factory: database::DatabaseConnectionFactory) -> (database::Connection, i64) {
        let conn = database::run_migrations(&factory).await.unwrap();
        let uuid = Uuid::parse_str("a1000000-0000-0000-0000-000000000001").unwrap();
        conn.execute(
            "INSERT INTO devices (uuid, lifecycle, architecture) VALUES (?1, 'new', 'x86-64')",
            (uuid,),
        )
        .await
        .unwrap();
        let device_id: i64 = conn
            .query_one("SELECT id FROM devices WHERE uuid = ?1", (uuid,), |r| {
                r.get(0)
            })
            .await
            .unwrap();
        (conn, device_id)
    }

    #[tokio::test]
    async fn test_create_and_list_warning() {
        let (conn, device_id) = setup(test_connection_factory!()).await;

        let warning = create_warning(&conn, device_id, "TEST_CODE", "A test warning")
            .await
            .unwrap();

        assert_eq!(warning.device_id, device_id);
        assert_eq!(warning.code, "TEST_CODE");
        assert_eq!(warning.message, "A test warning");
        assert!(warning.id > 0);

        let list = list_warnings(&conn, device_id).await.unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, warning.id);
        assert_eq!(list[0].code, "TEST_CODE");
    }

    #[tokio::test]
    async fn test_list_warnings_empty() {
        let (conn, device_id) = setup(test_connection_factory!()).await;

        let list = list_warnings(&conn, device_id).await.unwrap();
        assert!(list.is_empty());
    }

    #[tokio::test]
    async fn test_list_warnings_ordered_by_created_at() {
        let (conn, device_id) = setup(test_connection_factory!()).await;

        create_warning(&conn, device_id, "CODE_A", "First warning")
            .await
            .unwrap();
        create_warning(&conn, device_id, "CODE_B", "Second warning")
            .await
            .unwrap();

        let list = list_warnings(&conn, device_id).await.unwrap();
        assert_eq!(list.len(), 2);
        // Order must be ascending by created_at
        assert_eq!(list[0].code, "CODE_A");
        assert_eq!(list[1].code, "CODE_B");
    }

    #[tokio::test]
    async fn test_delete_warning_returns_true_when_found() {
        let (conn, device_id) = setup(test_connection_factory!()).await;

        let warning = create_warning(&conn, device_id, "CODE", "msg")
            .await
            .unwrap();

        let deleted = delete_warning(&conn, warning.id, device_id).await.unwrap();
        assert!(deleted);

        let list = list_warnings(&conn, device_id).await.unwrap();
        assert!(list.is_empty());
    }

    #[tokio::test]
    async fn test_delete_warning_returns_false_when_not_found() {
        let (conn, device_id) = setup(test_connection_factory!()).await;

        let deleted = delete_warning(&conn, 999, device_id).await.unwrap();
        assert!(!deleted);
    }

    #[tokio::test]
    async fn test_delete_warning_scoped_to_device() {
        let (conn, device_id_1) = setup(test_connection_factory!()).await;

        // Create a second device in the same db connection
        let uuid2 = Uuid::parse_str("b2000000-0000-0000-0000-000000000002").unwrap();
        conn.execute(
            "INSERT INTO devices (uuid, lifecycle, architecture) VALUES (?1, 'new', 'x86-64')",
            (uuid2,),
        )
        .await
        .unwrap();
        let device_id_2: i64 = conn
            .query_one("SELECT id FROM devices WHERE uuid = ?1", (uuid2,), |r| {
                r.get(0)
            })
            .await
            .unwrap();

        let warning = create_warning(&conn, device_id_1, "CODE", "msg")
            .await
            .unwrap();

        // Attempting to delete with wrong device_id should return false
        let deleted = delete_warning(&conn, warning.id, device_id_2)
            .await
            .unwrap();
        assert!(!deleted);

        // Warning still exists for the correct device
        let list = list_warnings(&conn, device_id_1).await.unwrap();
        assert_eq!(list.len(), 1);
    }

    #[tokio::test]
    async fn test_get_device_id_by_uuid() {
        let (conn, device_id) = setup(test_connection_factory!()).await;

        let uuid = Uuid::parse_str("c3000000-0000-0000-0000-000000000003").unwrap();
        conn.execute(
            "INSERT INTO devices (uuid, lifecycle, architecture) VALUES (?1, 'new', 'x86-64')",
            (uuid,),
        )
        .await
        .unwrap();

        let found_id = get_device_id_by_uuid(&conn, &uuid).await.unwrap();
        assert!(found_id.is_some());
        assert_ne!(found_id.unwrap(), device_id);
    }

    #[tokio::test]
    async fn test_get_device_id_by_uuid_not_found() {
        let (conn, _) = setup(test_connection_factory!()).await;

        // Use a UUID that was never inserted — parse_str is used since new_v4 requires the v4 feature
        let missing = Uuid::parse_str("ffffffff-ffff-ffff-ffff-ffffffffffff").unwrap();
        let found = get_device_id_by_uuid(&conn, &missing).await.unwrap();
        assert!(found.is_none());
    }

    #[tokio::test]
    async fn test_warning_cascade_delete_with_device() {
        let (conn, device_id) = setup(test_connection_factory!()).await;

        create_warning(&conn, device_id, "CODE", "msg")
            .await
            .unwrap();

        // Delete the device — cascades should remove the warning
        conn.execute("DELETE FROM devices WHERE id = ?1", (device_id,))
            .await
            .unwrap();

        let count: i64 = conn
            .query_one(
                "SELECT COUNT(*) FROM device_warnings WHERE device_id = ?1",
                (device_id,),
                |r| r.get(0),
            )
            .await
            .unwrap();
        assert_eq!(count, 0);
    }
}
