use super::{DiskLayout, Role, RoleWithOs};
use anyhow::{Context, Result, anyhow};
use chrono::Utc;

use crate::database::{Connection, FromRow};

/// Parameters for updating a role. All fields are optional; only `Some` values are applied.
pub struct UpdateRoleParams<'a> {
    pub name: Option<&'a str>,
    pub description: Option<&'a str>,
    pub os_id: Option<i64>,
    pub disk_layout: Option<&'a DiskLayout>,
    pub config_template: Option<&'a serde_json::Value>,
    pub firmware_mode: Option<common::FirmwareMode>,
    /// When true, sets firmware_mode to NULL regardless of the firmware_mode field.
    /// Takes precedence over firmware_mode when both are provided.
    pub clear_firmware_mode: bool,
}

/// Create a new role.
pub async fn create(
    conn: &Connection,
    name: &str,
    description: Option<&str>,
    os_id: i64,
    disk_layout: &DiskLayout,
    config_template: Option<&serde_json::Value>,
    firmware_mode: Option<common::FirmwareMode>,
) -> Result<Role> {
    let now = Utc::now();
    let disk_layout_json = serde_json::to_string(disk_layout)?;
    let config_json = config_template.map(serde_json::to_string).transpose()?;
    let firmware_mode_val = firmware_mode.map(|m| m.as_db_str());

    conn.execute(
        "INSERT INTO roles (name, description, os_id, disk_layout, config_template, firmware_mode, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        (name.to_string(), description.map(|s| s.to_string()), os_id, disk_layout_json, config_json, firmware_mode_val, now, now),
    )
    .await
    .context("Failed to insert role")?;

    let id = conn.last_insert_rowid().await;

    Ok(Role {
        id: Some(id),
        name: name.to_string(),
        description: description.map(|s| s.to_string()),
        os_id,
        disk_layout: disk_layout.clone(),
        config_template: config_template.cloned(),
        firmware_mode,
        created_at: Some(now),
        updated_at: Some(now),
    })
}

/// Get a role by ID.
pub async fn get(conn: &Connection, id: i64) -> Result<Role> {
    let role = conn
        .query_one(
            "SELECT id, name, description, os_id, disk_layout, config_template, firmware_mode, created_at, updated_at
             FROM roles WHERE id = ?1",
            (id,),
            Role::from_row,
        )
        .await
        .context("Role not found")?;

    Ok(role)
}

/// Get a role with its associated OS information.
pub async fn get_with_os(conn: &Connection, id: i64) -> Result<RoleWithOs> {
    let role = conn
        .query_one(
            "SELECT r.id, r.name, r.description, r.os_id, r.disk_layout, r.config_template,
                    r.firmware_mode, r.created_at, r.updated_at,
                    o.name AS os_name, o.version AS os_version
             FROM roles r
             JOIN operating_systems o ON r.os_id = o.id
             WHERE r.id = ?1",
            (id,),
            row_to_role_with_os,
        )
        .await
        .context("Role not found")?;

    Ok(role)
}

/// List all roles with their OS information.
pub async fn list_with_os(conn: &Connection) -> Result<Vec<RoleWithOs>> {
    let roles = conn
        .query(
            "SELECT r.id, r.name, r.description, r.os_id, r.disk_layout, r.config_template,
                    r.firmware_mode, r.created_at, r.updated_at,
                    o.name AS os_name, o.version AS os_version
             FROM roles r
             JOIN operating_systems o ON r.os_id = o.id
             ORDER BY r.name",
            (),
            row_to_role_with_os,
        )
        .await?;

    Ok(roles)
}

/// Map a database row from the roles-joined-with-OS query into a `RoleWithOs`.
///
/// Relies on named column access so callers are not sensitive to SELECT column order.
/// The query must alias `o.name` as `os_name` and `o.version` as `os_version`.
fn row_to_role_with_os(row: &rusqlite::Row) -> rusqlite::Result<RoleWithOs> {
    Ok(RoleWithOs {
        role: Role::from_row(row)?,
        os_name: row.get("os_name")?,
        os_version: row.get("os_version")?,
    })
}

/// Update a role.
pub async fn update(conn: &Connection, id: i64, params: UpdateRoleParams<'_>) -> Result<Role> {
    let now = Utc::now();

    let mut updates = Vec::new();
    let mut values: Vec<rusqlite::types::Value> = Vec::new();

    if let Some(name) = params.name {
        updates.push("name = ?");
        values.push(rusqlite::types::Value::Text(name.to_string()));
    }
    if let Some(description) = params.description {
        updates.push("description = ?");
        values.push(rusqlite::types::Value::Text(description.to_string()));
    }
    if let Some(os_id) = params.os_id {
        updates.push("os_id = ?");
        values.push(rusqlite::types::Value::Integer(os_id));
    }
    if let Some(disk_layout) = params.disk_layout {
        updates.push("disk_layout = ?");
        let json = serde_json::to_string(disk_layout)?;
        values.push(rusqlite::types::Value::Text(json));
    }
    if let Some(config_template) = params.config_template {
        updates.push("config_template = ?");
        let json = serde_json::to_string(config_template)?;
        values.push(rusqlite::types::Value::Text(json));
    }
    if params.clear_firmware_mode {
        updates.push("firmware_mode = NULL");
    } else if let Some(mode) = params.firmware_mode {
        updates.push("firmware_mode = ?");
        values.push(rusqlite::types::Value::Text(mode.as_db_str().to_string()));
    }

    if updates.is_empty() {
        return get(conn, id).await;
    }

    updates.push("updated_at = ?");
    values.push(rusqlite::types::Value::Text(now.to_rfc3339()));
    values.push(rusqlite::types::Value::Integer(id));

    let query = format!("UPDATE roles SET {} WHERE id = ?", updates.join(", "));

    conn.execute(query, rusqlite::params_from_iter(values))
        .await?;

    get(conn, id).await
}

/// Delete a role.
pub async fn delete(conn: &Connection, id: i64) -> Result<()> {
    let rows_affected = conn
        .execute("DELETE FROM roles WHERE id = ?1", (id,))
        .await
        .context("Failed to delete role")?;

    if rows_affected == 0 {
        return Err(anyhow!("Role not found"));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_database_path;
    use common::disk_layout::{DiskConfig, DiskLayout, PartitionConfig};

    async fn setup_db(path: String) -> Connection {
        let factory =
            crate::database::DatabaseConnectionFactory::new(std::path::PathBuf::from(path));
        crate::database::run_migrations(&factory).await.unwrap()
    }

    #[tokio::test]
    async fn test_create_and_get_role() {
        let db = setup_db(test_database_path!()).await;

        // Create OS first
        let os = crate::operating_systems::store::create(&db, "Ubuntu", "24.04", None)
            .await
            .unwrap();

        // Create role
        let disk_layout = DiskLayout {
            disks: vec![DiskConfig {
                device: "/dev/disk/by-path/pci-0000:00:1f.2-ata-1".to_string(),
                partition_table: "gpt".to_string(),
                partitions: vec![PartitionConfig {
                    label: "root".to_string(),
                    size: "100G".to_string(),
                    filesystem: Some("ext4".to_string()),
                    mount_point: Some("/".to_string()),
                    flags: None,
                    volume_group: None,
                }],
            }],
            volume_groups: None,
            zfs_pools: None,
        };

        let role = create(
            &db,
            "web-server",
            Some("Web server role"),
            os.id.unwrap(),
            &disk_layout,
            None,
            None,
        )
        .await
        .unwrap();

        assert!(role.id.is_some());
        assert_eq!(role.name, "web-server");

        let retrieved = get(&db, role.id.unwrap()).await.unwrap();
        assert_eq!(retrieved.name, role.name);
        assert_eq!(retrieved.disk_layout.disks.len(), 1);
        assert_eq!(retrieved.disk_layout.disks[0].partitions.len(), 1);
    }

    #[tokio::test]
    async fn test_list_roles() {
        let db = setup_db(test_database_path!()).await;

        let os = crate::operating_systems::store::create(&db, "Ubuntu", "24.04", None)
            .await
            .unwrap();
        let disk_layout = DiskLayout {
            disks: vec![],
            volume_groups: None,
            zfs_pools: None,
        };

        create(&db, "role1", None, os.id.unwrap(), &disk_layout, None, None)
            .await
            .unwrap();
        create(&db, "role2", None, os.id.unwrap(), &disk_layout, None, None)
            .await
            .unwrap();

        let list = list_with_os(&db).await.unwrap();
        assert_eq!(list.len(), 2);
    }

    #[tokio::test]
    async fn test_get_with_os() {
        let db = setup_db(test_database_path!()).await;

        let os = crate::operating_systems::store::create(&db, "Ubuntu", "24.04", None)
            .await
            .unwrap();
        let disk_layout = DiskLayout {
            disks: vec![],
            volume_groups: None,
            zfs_pools: None,
        };

        let role = create(
            &db,
            "web-server",
            None,
            os.id.unwrap(),
            &disk_layout,
            None,
            None,
        )
        .await
        .unwrap();

        let role_with_os = get_with_os(&db, role.id.unwrap()).await.unwrap();
        assert_eq!(role_with_os.os_name, "Ubuntu");
        assert_eq!(role_with_os.os_version, "24.04");
    }

    #[tokio::test]
    async fn test_update_role() {
        let db = setup_db(test_database_path!()).await;

        let os = crate::operating_systems::store::create(&db, "Ubuntu", "24.04", None)
            .await
            .unwrap();
        let disk_layout = DiskLayout {
            disks: vec![],
            volume_groups: None,
            zfs_pools: None,
        };

        let role = create(
            &db,
            "web-server",
            None,
            os.id.unwrap(),
            &disk_layout,
            None,
            None,
        )
        .await
        .unwrap();

        let updated = update(
            &db,
            role.id.unwrap(),
            UpdateRoleParams {
                name: Some("updated-name"),
                description: Some("New description"),
                os_id: None,
                disk_layout: None,
                config_template: None,
                firmware_mode: None,
                clear_firmware_mode: false,
            },
        )
        .await
        .unwrap();

        assert_eq!(updated.name, "updated-name");
        assert_eq!(updated.description, Some("New description".to_string()));
    }

    #[tokio::test]
    async fn test_delete_role() {
        let db = setup_db(test_database_path!()).await;

        let os = crate::operating_systems::store::create(&db, "Ubuntu", "24.04", None)
            .await
            .unwrap();
        let disk_layout = DiskLayout {
            disks: vec![],
            volume_groups: None,
            zfs_pools: None,
        };

        let role = create(
            &db,
            "web-server",
            None,
            os.id.unwrap(),
            &disk_layout,
            None,
            None,
        )
        .await
        .unwrap();

        delete(&db, role.id.unwrap()).await.unwrap();
        assert!(get(&db, role.id.unwrap()).await.is_err());
    }

    #[tokio::test]
    async fn test_create_role_with_uefi_firmware_mode() {
        let db = setup_db(test_database_path!()).await;

        let os = crate::operating_systems::store::create(&db, "Ubuntu", "24.04", None)
            .await
            .unwrap();
        let disk_layout = DiskLayout {
            disks: vec![],
            volume_groups: None,
            zfs_pools: None,
        };

        let role = create(
            &db,
            "uefi-role",
            None,
            os.id.unwrap(),
            &disk_layout,
            None,
            Some(common::FirmwareMode::Uefi),
        )
        .await
        .unwrap();

        assert_eq!(role.firmware_mode, Some(common::FirmwareMode::Uefi));

        let retrieved = get(&db, role.id.unwrap()).await.unwrap();
        assert_eq!(retrieved.firmware_mode, Some(common::FirmwareMode::Uefi));
    }

    #[tokio::test]
    async fn test_create_role_with_bios_firmware_mode() {
        let db = setup_db(test_database_path!()).await;

        let os = crate::operating_systems::store::create(&db, "Ubuntu", "24.04", None)
            .await
            .unwrap();
        let disk_layout = DiskLayout {
            disks: vec![],
            volume_groups: None,
            zfs_pools: None,
        };

        let role = create(
            &db,
            "bios-role",
            None,
            os.id.unwrap(),
            &disk_layout,
            None,
            Some(common::FirmwareMode::Bios),
        )
        .await
        .unwrap();

        assert_eq!(role.firmware_mode, Some(common::FirmwareMode::Bios));

        let retrieved = get(&db, role.id.unwrap()).await.unwrap();
        assert_eq!(retrieved.firmware_mode, Some(common::FirmwareMode::Bios));
    }

    #[tokio::test]
    async fn test_create_role_without_firmware_mode() {
        let db = setup_db(test_database_path!()).await;

        let os = crate::operating_systems::store::create(&db, "Ubuntu", "24.04", None)
            .await
            .unwrap();
        let disk_layout = DiskLayout {
            disks: vec![],
            volume_groups: None,
            zfs_pools: None,
        };

        let role = create(
            &db,
            "no-firmware-mode-role",
            None,
            os.id.unwrap(),
            &disk_layout,
            None,
            None,
        )
        .await
        .unwrap();

        assert!(role.firmware_mode.is_none());

        let retrieved = get(&db, role.id.unwrap()).await.unwrap();
        assert!(retrieved.firmware_mode.is_none());
    }

    #[tokio::test]
    async fn test_update_role_firmware_mode() {
        let db = setup_db(test_database_path!()).await;

        let os = crate::operating_systems::store::create(&db, "Ubuntu", "24.04", None)
            .await
            .unwrap();
        let disk_layout = DiskLayout {
            disks: vec![],
            volume_groups: None,
            zfs_pools: None,
        };

        let role = create(&db, "role", None, os.id.unwrap(), &disk_layout, None, None)
            .await
            .unwrap();

        let updated = update(
            &db,
            role.id.unwrap(),
            UpdateRoleParams {
                name: None,
                description: None,
                os_id: None,
                disk_layout: None,
                config_template: None,
                firmware_mode: Some(common::FirmwareMode::Uefi),
                clear_firmware_mode: false,
            },
        )
        .await
        .unwrap();

        assert_eq!(updated.firmware_mode, Some(common::FirmwareMode::Uefi));

        let retrieved = get(&db, role.id.unwrap()).await.unwrap();
        assert_eq!(retrieved.firmware_mode, Some(common::FirmwareMode::Uefi));
    }

    #[tokio::test]
    async fn test_clear_firmware_mode_sets_null() {
        let db = setup_db(test_database_path!()).await;

        let os = crate::operating_systems::store::create(&db, "Ubuntu", "24.04", None)
            .await
            .unwrap();
        let disk_layout = DiskLayout {
            disks: vec![],
            volume_groups: None,
            zfs_pools: None,
        };

        // Create a role that starts with a firmware_mode set.
        let role = create(
            &db,
            "role",
            None,
            os.id.unwrap(),
            &disk_layout,
            None,
            Some(common::FirmwareMode::Uefi),
        )
        .await
        .unwrap();

        assert_eq!(role.firmware_mode, Some(common::FirmwareMode::Uefi));

        // clear_firmware_mode should set firmware_mode to NULL.
        let updated = update(
            &db,
            role.id.unwrap(),
            UpdateRoleParams {
                name: None,
                description: None,
                os_id: None,
                disk_layout: None,
                config_template: None,
                firmware_mode: None,
                clear_firmware_mode: true,
            },
        )
        .await
        .unwrap();

        assert!(
            updated.firmware_mode.is_none(),
            "firmware_mode should be cleared to None"
        );

        let retrieved = get(&db, role.id.unwrap()).await.unwrap();
        assert!(
            retrieved.firmware_mode.is_none(),
            "firmware_mode should be None after clear"
        );
    }

    #[tokio::test]
    async fn test_clear_firmware_mode_takes_precedence_over_firmware_mode_field() {
        let db = setup_db(test_database_path!()).await;

        let os = crate::operating_systems::store::create(&db, "Ubuntu", "24.04", None)
            .await
            .unwrap();
        let disk_layout = DiskLayout {
            disks: vec![],
            volume_groups: None,
            zfs_pools: None,
        };

        let role = create(
            &db,
            "role",
            None,
            os.id.unwrap(),
            &disk_layout,
            None,
            Some(common::FirmwareMode::Uefi),
        )
        .await
        .unwrap();

        // When clear_firmware_mode is true, firmware_mode field is ignored.
        let updated = update(
            &db,
            role.id.unwrap(),
            UpdateRoleParams {
                name: None,
                description: None,
                os_id: None,
                disk_layout: None,
                config_template: None,
                firmware_mode: Some(common::FirmwareMode::Bios),
                clear_firmware_mode: true,
            },
        )
        .await
        .unwrap();

        assert!(
            updated.firmware_mode.is_none(),
            "clear_firmware_mode=true must override the firmware_mode field"
        );
    }
}
