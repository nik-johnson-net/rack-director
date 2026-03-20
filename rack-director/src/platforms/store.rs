use super::{Platform, PlatformAttributes};
use anyhow::{Context, Result, anyhow};
use chrono::Utc;

use crate::database::{Connection, FromRow};

/// Typed error returned by [`update_disk_label`].
///
/// Using a typed error allows callers to branch on error kind without fragile
/// string matching on the error message.
#[derive(Debug)]
pub enum UpdateDiskLabelError {
    /// No platform with the given ID exists.
    PlatformNotFound,
    /// The supplied disk index is out of bounds for this platform's disk list.
    IndexOutOfBounds,
    /// The requested label is already assigned to a different disk in this platform.
    DuplicateLabel,
    /// An unexpected database or serialization error occurred.
    Other(anyhow::Error),
}

impl std::fmt::Display for UpdateDiskLabelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UpdateDiskLabelError::PlatformNotFound => write!(f, "Platform not found"),
            UpdateDiskLabelError::IndexOutOfBounds => write!(f, "Disk index out of bounds"),
            UpdateDiskLabelError::DuplicateLabel => {
                write!(f, "Label already exists on another disk")
            }
            UpdateDiskLabelError::Other(e) => write!(f, "{}", e),
        }
    }
}

impl std::error::Error for UpdateDiskLabelError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            UpdateDiskLabelError::Other(e) => Some(e.as_ref()),
            _ => None,
        }
    }
}

impl From<anyhow::Error> for UpdateDiskLabelError {
    fn from(e: anyhow::Error) -> Self {
        UpdateDiskLabelError::Other(e)
    }
}

/// Create a new platform.
pub async fn create(
    conn: &Connection,
    name: &str,
    description: Option<&str>,
    attributes: &PlatformAttributes,
    firmware_mode: Option<common::FirmwareMode>,
) -> Result<Platform> {
    let now = Utc::now();
    let attributes_json = serde_json::to_string(attributes)?;
    let firmware_mode_val = firmware_mode.map(|m| m.as_db_str());

    conn.execute(
        "INSERT INTO platforms (name, description, attributes, firmware_mode, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        (
            name.to_string(),
            description.map(|s| s.to_string()),
            attributes_json,
            firmware_mode_val,
            now,
            now,
        ),
    )
    .await
    .context("Failed to insert platform")?;

    let id = conn.last_insert_rowid().await;

    Ok(Platform {
        id: Some(id),
        name: name.to_string(),
        description: description.map(|s| s.to_string()),
        attributes: attributes.clone(),
        firmware_mode,
        created_at: Some(now),
        updated_at: Some(now),
    })
}

/// Get a platform by ID.
pub async fn get(conn: &Connection, id: i64) -> Result<Platform> {
    let platform = conn
        .query_one(
            "SELECT id, name, description, attributes, firmware_mode, created_at, updated_at
             FROM platforms WHERE id = ?1",
            (id,),
            Platform::from_row,
        )
        .await
        .context("Platform not found")?;

    Ok(platform)
}

/// List all platforms.
pub async fn list(conn: &Connection) -> Result<Vec<Platform>> {
    let platforms = conn
        .query(
            "SELECT id, name, description, attributes, firmware_mode, created_at, updated_at
             FROM platforms ORDER BY name",
            (),
            Platform::from_row,
        )
        .await?;

    Ok(platforms)
}

/// Update a platform.
pub async fn update(
    conn: &Connection,
    id: i64,
    name: Option<&str>,
    description: Option<&str>,
    attributes: Option<&PlatformAttributes>,
    firmware_mode: Option<common::FirmwareMode>,
) -> Result<Platform> {
    let now = Utc::now();

    let mut updates = Vec::new();
    let mut values: Vec<rusqlite::types::Value> = Vec::new();

    if let Some(name) = name {
        updates.push("name = ?");
        values.push(rusqlite::types::Value::Text(name.to_string()));
    }
    if let Some(description) = description {
        updates.push("description = ?");
        values.push(rusqlite::types::Value::Text(description.to_string()));
    }
    if let Some(attributes) = attributes {
        updates.push("attributes = ?");
        let json = serde_json::to_string(attributes)?;
        values.push(rusqlite::types::Value::Text(json));
    }
    if let Some(mode) = firmware_mode {
        updates.push("firmware_mode = ?");
        values.push(rusqlite::types::Value::Text(mode.as_db_str().to_string()));
    }

    if updates.is_empty() {
        return get(conn, id).await;
    }

    updates.push("updated_at = ?");
    values.push(rusqlite::types::Value::Text(now.to_rfc3339()));
    values.push(rusqlite::types::Value::Integer(id));

    let query = format!("UPDATE platforms SET {} WHERE id = ?", updates.join(", "));

    conn.execute(query, rusqlite::params_from_iter(values))
        .await?;

    get(conn, id).await
}

/// Update the label on a single disk by zero-based index.
///
/// Loads the current platform attributes, validates that the requested label
/// does not already exist on a *different* disk (setting the same label on the
/// same disk is a no-op and is permitted), persists the change, and returns
/// the updated platform.
///
/// # Errors
///
/// * Returns [`UpdateDiskLabelError::PlatformNotFound`] if `id` does not match any platform.
/// * Returns [`UpdateDiskLabelError::IndexOutOfBounds`] if `index >= disks.len()`.
/// * Returns [`UpdateDiskLabelError::DuplicateLabel`] if `label` is already assigned to a
///   different disk within this platform.
/// * Returns [`UpdateDiskLabelError::Other`] for unexpected database or serialization errors.
pub async fn update_disk_label(
    conn: &Connection,
    id: i64,
    index: usize,
    label: Option<&str>,
) -> std::result::Result<Platform, UpdateDiskLabelError> {
    let platform = get(conn, id)
        .await
        .map_err(|_| UpdateDiskLabelError::PlatformNotFound)?;

    let mut attributes = platform.attributes.clone();

    if index >= attributes.disks.len() {
        return Err(UpdateDiskLabelError::IndexOutOfBounds);
    }

    validate_label_uniqueness(&attributes, index, label)
        .map_err(|_| UpdateDiskLabelError::DuplicateLabel)?;

    attributes.disks[index].label = label.map(|s| s.to_string());

    let now = Utc::now();
    let attributes_json = serde_json::to_string(&attributes).map_err(anyhow::Error::from)?;

    conn.execute(
        "UPDATE platforms SET attributes = ?1, updated_at = ?2 WHERE id = ?3",
        (attributes_json, now, id),
    )
    .await
    .context("Failed to update disk label")?;

    get(conn, id).await.map_err(Into::into)
}

/// Check that `label` is not already assigned to a disk at a different index.
///
/// A `None` label (clearing) is always valid. Setting the same label on the
/// same index is also valid (idempotent update).
fn validate_label_uniqueness(
    attributes: &PlatformAttributes,
    target_index: usize,
    label: Option<&str>,
) -> Result<()> {
    let Some(new_label) = label else {
        return Ok(());
    };

    for (i, disk) in attributes.disks.iter().enumerate() {
        if i == target_index {
            continue;
        }
        if disk.label.as_deref() == Some(new_label) {
            return Err(anyhow!("Label already exists on another disk"));
        }
    }

    Ok(())
}

/// Delete a platform.
///
/// Returns an error if devices are assigned to this platform.
pub async fn delete(conn: &Connection, id: i64) -> Result<()> {
    // Atomic check-and-delete: only deletes if no devices are assigned.
    // Using NOT EXISTS in the WHERE clause makes the check and delete a single
    // atomic SQL statement, preventing race conditions.
    let rows_affected = conn
        .execute(
            "DELETE FROM platforms WHERE id = ?1 AND NOT EXISTS (SELECT 1 FROM devices WHERE platform_id = ?1)",
            (id,),
        )
        .await
        .context("Failed to delete platform")?;

    if rows_affected == 0 {
        // Either platform doesn't exist, or devices are assigned — distinguish for caller
        let device_count: i64 = conn
            .query_one(
                "SELECT COUNT(*) FROM devices WHERE platform_id = ?1",
                (id,),
                |row| row.get(0),
            )
            .await
            .unwrap_or(0);

        if device_count > 0 {
            return Err(anyhow!(
                "Cannot delete platform: {} device(s) are assigned to it",
                device_count
            ));
        }
        return Err(anyhow!("Platform not found"));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::operating_systems::Architecture;
    use crate::platforms::{DiskType, PlatformCpu, PlatformDisk, PlatformNic};
    use crate::test_database_path;
    use uuid::Uuid;

    async fn setup_db(path: String) -> Connection {
        let factory =
            crate::database::DatabaseConnectionFactory::new(std::path::PathBuf::from(path));
        crate::database::run_migrations(&factory).await.unwrap()
    }

    fn sample_platform_attributes() -> PlatformAttributes {
        PlatformAttributes {
            disks: vec![
                PlatformDisk {
                    size_gb: 480,
                    disk_type: DiskType::Ssd,
                    label: Some("ROOT".to_string()),
                },
                PlatformDisk {
                    size_gb: 2000,
                    disk_type: DiskType::Hdd,
                    label: Some("DATA1".to_string()),
                },
            ],
            nics: vec![
                PlatformNic {
                    logical: "eno1".to_string(),
                    speed_mbps: Some(10000),
                    label: Some("NIC1".to_string()),
                },
                PlatformNic {
                    logical: "eno2".to_string(),
                    speed_mbps: Some(10000),
                    label: Some("NIC2".to_string()),
                },
            ],
            cpus: vec![PlatformCpu {
                brand: "intel".to_string(),
                model: "E3-1240 v3".to_string(),
                cores: 4,
            }],
            memory_gib: 32,
        }
    }

    #[tokio::test]
    async fn test_create_and_get_platform() {
        let db = setup_db(test_database_path!()).await;

        let attrs = sample_platform_attributes();
        let platform = create(&db, "PowerEdge R640", Some("Dell server"), &attrs, None)
            .await
            .unwrap();

        assert!(platform.id.is_some());
        assert_eq!(platform.name, "PowerEdge R640");
        assert_eq!(platform.description, Some("Dell server".to_string()));
        assert_eq!(platform.attributes.disks.len(), 2);
        assert_eq!(platform.attributes.nics.len(), 2);
        assert_eq!(platform.attributes.cpus.len(), 1);
        assert_eq!(platform.attributes.memory_gib, 32);

        let retrieved = get(&db, platform.id.unwrap()).await.unwrap();
        assert_eq!(retrieved.name, platform.name);
        assert_eq!(retrieved.attributes.disks.len(), 2);
    }

    #[tokio::test]
    async fn test_list_platforms() {
        let db = setup_db(test_database_path!()).await;

        let attrs = sample_platform_attributes();
        create(&db, "Platform 1", None, &attrs, None).await.unwrap();
        create(&db, "Platform 2", None, &attrs, None).await.unwrap();

        let platforms = list(&db).await.unwrap();
        assert_eq!(platforms.len(), 2);
    }

    #[tokio::test]
    async fn test_update_platform() {
        let db = setup_db(test_database_path!()).await;

        let attrs = sample_platform_attributes();
        let platform = create(&db, "Original Name", None, &attrs, None).await.unwrap();

        let updated = update(
            &db,
            platform.id.unwrap(),
            Some("Updated Name"),
            Some("New description"),
            None,
            None,
        )
        .await
        .unwrap();

        assert_eq!(updated.name, "Updated Name");
        assert_eq!(updated.description, Some("New description".to_string()));
    }

    #[tokio::test]
    async fn test_delete_platform() {
        let db = setup_db(test_database_path!()).await;

        let attrs = sample_platform_attributes();
        let platform = create(&db, "Test Platform", None, &attrs, None).await.unwrap();

        delete(&db, platform.id.unwrap()).await.unwrap();
        assert!(get(&db, platform.id.unwrap()).await.is_err());
    }

    #[tokio::test]
    async fn test_delete_platform_with_devices_fails() {
        let db = setup_db(test_database_path!()).await;

        // Create platform
        let attrs = sample_platform_attributes();
        let platform = create(&db, "Test Platform", None, &attrs, None).await.unwrap();

        // Create device and assign platform
        let device_uuid = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440020").unwrap();
        db.execute(
            "INSERT INTO devices (uuid, lifecycle, architecture) VALUES (?1, 'new', ?2)",
            (device_uuid, Architecture::X86_64.as_str().to_string()),
        )
        .await
        .unwrap();

        crate::director::store::assign_platform_to_device(&db, &device_uuid, platform.id.unwrap())
            .await
            .unwrap();

        // Try to delete platform - should fail
        let result = delete(&db, platform.id.unwrap()).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Cannot delete"));
    }

    #[tokio::test]
    async fn test_concurrent_delete_and_assign_protection() {
        use std::sync::Arc;

        let db = Arc::new(setup_db(test_database_path!()).await);

        // Create platform
        let attrs = sample_platform_attributes();
        let platform = create(&db, "Test Platform", None, &attrs, None).await.unwrap();
        let platform_id = platform.id.unwrap();

        // Create device (without assigning platform yet)
        let device_uuid = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440021").unwrap();
        db.execute(
            "INSERT INTO devices (uuid, lifecycle, architecture) VALUES (?1, 'new', ?2)",
            (device_uuid, Architecture::X86_64.as_str().to_string()),
        )
        .await
        .unwrap();

        // Spawn two concurrent tasks that race to complete:
        // Task A: Delete the platform
        // Task B: Assign the platform to the device
        let db_delete = db.clone();
        let db_assign = db.clone();

        let delete_task = tokio::spawn(async move {
            tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
            delete(&db_delete, platform_id).await
        });

        let assign_task = tokio::spawn(async move {
            tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
            crate::director::store::assign_platform_to_device(&db_assign, &device_uuid, platform_id)
                .await
        });

        let (delete_result, assign_result) = tokio::join!(delete_task, assign_task);

        let delete_result = delete_result.expect("Delete task panicked");
        let assign_result = assign_result.expect("Assign task panicked");

        match (delete_result, assign_result) {
            (Ok(_), Err(_)) => {
                let platform_check = get(&db, platform_id).await;
                assert!(
                    platform_check.is_err(),
                    "Platform should not exist after successful delete"
                );
            }
            (Err(e), Ok(_)) => {
                let err_msg = e.to_string();
                assert!(
                    err_msg.contains("Cannot delete") || err_msg.contains("device(s) are assigned"),
                    "Delete should fail with correct error message, got: {}",
                    err_msg
                );

                let device_platform =
                    crate::director::store::get_device_platform_id(&db, &device_uuid)
                        .await
                        .unwrap();
                assert_eq!(
                    device_platform,
                    Some(platform_id),
                    "Device should have platform assigned"
                );
            }
            (Ok(_), Ok(_)) => {
                panic!(
                    "CRITICAL: Both delete and assign succeeded - transaction protection failed!"
                );
            }
            (Err(e1), Err(e2)) => {
                panic!(
                    "Both operations failed unexpectedly:\nDelete error: {}\nAssign error: {}",
                    e1, e2
                );
            }
        }
    }

    #[tokio::test]
    async fn test_update_disk_label_renames_label() {
        let db = setup_db(test_database_path!()).await;

        let attrs = sample_platform_attributes();
        let platform = create(&db, "Test Platform", None, &attrs, None)
            .await
            .unwrap();
        let id = platform.id.unwrap();

        let updated = update_disk_label(&db, id, 1, Some("CACHE")).await.unwrap();
        assert_eq!(updated.attributes.disks[1].label, Some("CACHE".to_string()));
        // Other disks must be unchanged
        assert_eq!(updated.attributes.disks[0].label, Some("ROOT".to_string()));

        // Persisted to DB
        let reloaded = get(&db, id).await.unwrap();
        assert_eq!(
            reloaded.attributes.disks[1].label,
            Some("CACHE".to_string())
        );
    }

    #[tokio::test]
    async fn test_update_disk_label_clears_label() {
        let db = setup_db(test_database_path!()).await;

        let attrs = sample_platform_attributes();
        let platform = create(&db, "Test Platform", None, &attrs, None)
            .await
            .unwrap();
        let id = platform.id.unwrap();

        let updated = update_disk_label(&db, id, 0, None).await.unwrap();
        assert_eq!(updated.attributes.disks[0].label, None);

        let reloaded = get(&db, id).await.unwrap();
        assert_eq!(reloaded.attributes.disks[0].label, None);
    }

    #[tokio::test]
    async fn test_update_disk_label_same_label_on_same_disk_is_allowed() {
        let db = setup_db(test_database_path!()).await;

        let attrs = sample_platform_attributes();
        let platform = create(&db, "Test Platform", None, &attrs, None)
            .await
            .unwrap();
        let id = platform.id.unwrap();

        // Setting "ROOT" on index 0 again is a no-op and must not error
        let updated = update_disk_label(&db, id, 0, Some("ROOT")).await.unwrap();
        assert_eq!(updated.attributes.disks[0].label, Some("ROOT".to_string()));
    }

    #[tokio::test]
    async fn test_update_disk_label_duplicate_on_other_disk_is_rejected() {
        let db = setup_db(test_database_path!()).await;

        let attrs = sample_platform_attributes();
        let platform = create(&db, "Test Platform", None, &attrs, None)
            .await
            .unwrap();
        let id = platform.id.unwrap();

        // "ROOT" already belongs to disk 0; assigning it to disk 1 must fail
        let result = update_disk_label(&db, id, 1, Some("ROOT")).await;
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("Label already exists"),
            "Expected duplicate-label error, got: {msg}"
        );
    }

    #[tokio::test]
    async fn test_update_disk_label_index_out_of_bounds() {
        let db = setup_db(test_database_path!()).await;

        let attrs = sample_platform_attributes();
        let platform = create(&db, "Test Platform", None, &attrs, None)
            .await
            .unwrap();
        let id = platform.id.unwrap();

        let result = update_disk_label(&db, id, 99, Some("EXTRA")).await;
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("out of bounds"),
            "Expected out-of-bounds error, got: {msg}"
        );
    }

    #[tokio::test]
    async fn test_update_disk_label_platform_not_found() {
        let db = setup_db(test_database_path!()).await;

        let result = update_disk_label(&db, 9999, 0, Some("ROOT")).await;
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("Platform not found"),
            "Expected not-found error, got: {msg}"
        );
    }

    #[tokio::test]
    async fn test_create_platform_with_firmware_mode() {
        let db = setup_db(test_database_path!()).await;

        let attrs = sample_platform_attributes();
        let platform = create(
            &db,
            "UEFI Platform",
            None,
            &attrs,
            Some(common::FirmwareMode::Uefi),
        )
        .await
        .unwrap();

        assert_eq!(platform.firmware_mode, Some(common::FirmwareMode::Uefi));

        let retrieved = get(&db, platform.id.unwrap()).await.unwrap();
        assert_eq!(retrieved.firmware_mode, Some(common::FirmwareMode::Uefi));
    }

    #[tokio::test]
    async fn test_create_platform_with_bios_firmware_mode() {
        let db = setup_db(test_database_path!()).await;

        let attrs = sample_platform_attributes();
        let platform = create(
            &db,
            "BIOS Platform",
            None,
            &attrs,
            Some(common::FirmwareMode::Bios),
        )
        .await
        .unwrap();

        assert_eq!(platform.firmware_mode, Some(common::FirmwareMode::Bios));

        let retrieved = get(&db, platform.id.unwrap()).await.unwrap();
        assert_eq!(retrieved.firmware_mode, Some(common::FirmwareMode::Bios));
    }

    #[tokio::test]
    async fn test_create_platform_without_firmware_mode() {
        let db = setup_db(test_database_path!()).await;

        let attrs = sample_platform_attributes();
        let platform = create(&db, "No Firmware Mode", None, &attrs, None)
            .await
            .unwrap();

        assert!(platform.firmware_mode.is_none());

        let retrieved = get(&db, platform.id.unwrap()).await.unwrap();
        assert!(retrieved.firmware_mode.is_none());
    }

    #[tokio::test]
    async fn test_update_platform_firmware_mode() {
        let db = setup_db(test_database_path!()).await;

        let attrs = sample_platform_attributes();
        let platform = create(&db, "Platform", None, &attrs, None).await.unwrap();

        let updated = update(
            &db,
            platform.id.unwrap(),
            None,
            None,
            None,
            Some(common::FirmwareMode::Uefi),
        )
        .await
        .unwrap();

        assert_eq!(updated.firmware_mode, Some(common::FirmwareMode::Uefi));

        let retrieved = get(&db, platform.id.unwrap()).await.unwrap();
        assert_eq!(retrieved.firmware_mode, Some(common::FirmwareMode::Uefi));
    }
}
