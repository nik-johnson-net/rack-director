use super::{DiskLayout, Role};
use anyhow::{Context, Result, anyhow};
use chrono::Utc;

use crate::database::{Connection, FromRow, text_or_null};

/// Parameters for updating a role. `None` leaves a column unchanged.
///
/// Nullable columns are doubly wrapped: `Some(None)` sets the column to `NULL`,
/// `Some(Some(v))` writes `v`.
#[derive(Default)]
pub struct UpdateRoleParams<'a> {
    pub name: Option<&'a str>,
    pub description: Option<Option<&'a str>>,
    pub osm_module: Option<&'a str>,
    pub os_name: Option<&'a str>,
    pub os_release: Option<&'a str>,
    pub os_arch: Option<&'a str>,
    pub disk_layout: Option<&'a DiskLayout>,
    pub cmdline_args: Option<Option<&'a str>>,
    pub config_template: Option<Option<&'a serde_json::Value>>,
    pub firmware_mode: Option<Option<common::FirmwareMode>>,
}

/// Create a new role.
pub async fn create(
    conn: &Connection,
    name: &str,
    description: Option<&str>,
    osm_module: &str,
    os_name: &str,
    os_release: &str,
    os_arch: &str,
    disk_layout: &DiskLayout,
    cmdline_args: Option<&str>,
    config_template: Option<&serde_json::Value>,
    firmware_mode: Option<common::FirmwareMode>,
) -> Result<Role> {
    let now = Utc::now();
    let disk_layout_json = serde_json::to_string(disk_layout)?;
    let config_json = config_template.map(serde_json::to_string).transpose()?;
    let firmware_mode_val = firmware_mode.map(|m| m.as_db_str());

    conn.execute(
        "INSERT INTO roles (name, description, osm_module, os_name, os_release, os_arch, disk_layout, cmdline_args, config_template, firmware_mode, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        (
            name.to_string(),
            description.map(|s| s.to_string()),
            osm_module.to_string(),
            os_name.to_string(),
            os_release.to_string(),
            os_arch.to_string(),
            disk_layout_json,
            cmdline_args.map(|s| s.to_string()),
            config_json,
            firmware_mode_val,
            now,
            now,
        ),
    )
    .await
    .context("Failed to insert role")?;

    let id = conn.last_insert_rowid().await;

    Ok(Role {
        id: Some(id),
        name: name.to_string(),
        description: description.map(|s| s.to_string()),
        osm_module: osm_module.to_string(),
        os_name: os_name.to_string(),
        os_release: os_release.to_string(),
        os_arch: os_arch.to_string(),
        disk_layout: disk_layout.clone(),
        cmdline_args: cmdline_args.map(|s| s.to_string()),
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
            "SELECT id, name, description, osm_module, os_name, os_release, os_arch,
                    disk_layout, cmdline_args, config_template, firmware_mode, created_at, updated_at
             FROM roles WHERE id = ?1",
            (id,),
            Role::from_row,
        )
        .await
        .context("Role not found")?;

    Ok(role)
}

/// List all roles.
pub async fn list(conn: &Connection) -> Result<Vec<Role>> {
    let roles = conn
        .query(
            "SELECT id, name, description, osm_module, os_name, os_release, os_arch,
                    disk_layout, cmdline_args, config_template, firmware_mode, created_at, updated_at
             FROM roles ORDER BY name",
            (),
            Role::from_row,
        )
        .await?;
    Ok(roles)
}

/// Collect the `SET` clauses and bound values for the columns being changed.
///
/// Returns `(clauses, values)` with one `?` placeholder per value, in matching order.
fn build_role_updates(
    params: &UpdateRoleParams<'_>,
) -> Result<(Vec<&'static str>, Vec<rusqlite::types::Value>)> {
    let mut updates = Vec::new();
    let mut values: Vec<rusqlite::types::Value> = Vec::new();

    if let Some(name) = params.name {
        updates.push("name = ?");
        values.push(rusqlite::types::Value::Text(name.to_string()));
    }
    if let Some(description) = params.description {
        updates.push("description = ?");
        values.push(text_or_null(description));
    }
    if let Some(osm_module) = params.osm_module {
        updates.push("osm_module = ?");
        values.push(rusqlite::types::Value::Text(osm_module.to_string()));
    }
    if let Some(os_name) = params.os_name {
        updates.push("os_name = ?");
        values.push(rusqlite::types::Value::Text(os_name.to_string()));
    }
    if let Some(os_release) = params.os_release {
        updates.push("os_release = ?");
        values.push(rusqlite::types::Value::Text(os_release.to_string()));
    }
    if let Some(os_arch) = params.os_arch {
        updates.push("os_arch = ?");
        values.push(rusqlite::types::Value::Text(os_arch.to_string()));
    }
    if let Some(disk_layout) = params.disk_layout {
        updates.push("disk_layout = ?");
        let json = serde_json::to_string(disk_layout)?;
        values.push(rusqlite::types::Value::Text(json));
    }
    if let Some(cmdline_args) = params.cmdline_args {
        updates.push("cmdline_args = ?");
        values.push(text_or_null(cmdline_args));
    }
    if let Some(config_template) = params.config_template {
        updates.push("config_template = ?");
        let json = config_template.map(serde_json::to_string).transpose()?;
        values.push(text_or_null(json.as_deref()));
    }
    if let Some(firmware_mode) = params.firmware_mode {
        updates.push("firmware_mode = ?");
        values.push(text_or_null(firmware_mode.map(|m| m.as_db_str())));
    }

    Ok((updates, values))
}

/// Update a role.
pub async fn update(conn: &Connection, id: i64, params: UpdateRoleParams<'_>) -> Result<Role> {
    let now = Utc::now();

    let (mut updates, mut values) = build_role_updates(&params)?;

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
    use crate::{database::DatabaseConnectionFactory, test_connection_factory};
    use common::disk_layout::{DiskConfig, DiskLayout, PartitionConfig};

    async fn setup_db(factory: DatabaseConnectionFactory) -> Connection {
        crate::database::run_migrations(&factory).await.unwrap()
    }

    fn simple_disk_layout() -> DiskLayout {
        DiskLayout {
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
            wipe_all_disks: false,
        }
    }

    fn empty_disk_layout() -> DiskLayout {
        DiskLayout {
            disks: vec![],
            volume_groups: None,
            zfs_pools: None,
            wipe_all_disks: false,
        }
    }

    #[tokio::test]
    async fn test_create_and_get_role() {
        let db = setup_db(test_connection_factory!()).await;

        let role = create(
            &db,
            "web-server",
            Some("Web server role"),
            "Default",
            "Ubuntu",
            "24.04",
            "x86-64",
            &simple_disk_layout(),
            None,
            None,
            None,
        )
        .await
        .unwrap();

        assert!(role.id.is_some());
        assert_eq!(role.name, "web-server");
        assert_eq!(role.osm_module, "Default");
        assert_eq!(role.os_name, "Ubuntu");
        assert_eq!(role.os_release, "24.04");
        assert_eq!(role.os_arch, "x86-64");

        let retrieved = get(&db, role.id.unwrap()).await.unwrap();
        assert_eq!(retrieved.name, role.name);
        assert_eq!(retrieved.osm_module, "Default");
        assert_eq!(retrieved.os_name, "Ubuntu");
        assert_eq!(retrieved.os_release, "24.04");
        assert_eq!(retrieved.os_arch, "x86-64");
        assert_eq!(retrieved.disk_layout.disks.len(), 1);
        assert_eq!(retrieved.disk_layout.disks[0].partitions.len(), 1);
    }

    #[tokio::test]
    async fn test_list_roles() {
        let db = setup_db(test_connection_factory!()).await;

        create(
            &db,
            "role1",
            None,
            "Default",
            "Ubuntu",
            "22.04",
            "x86-64",
            &empty_disk_layout(),
            None,
            None,
            None,
        )
        .await
        .unwrap();
        create(
            &db,
            "role2",
            None,
            "Default",
            "Ubuntu",
            "24.04",
            "x86-64",
            &empty_disk_layout(),
            None,
            None,
            None,
        )
        .await
        .unwrap();

        let roles = list(&db).await.unwrap();
        assert_eq!(roles.len(), 2);
    }

    #[tokio::test]
    async fn test_role_osm_fields_roundtrip() {
        let db = setup_db(test_connection_factory!()).await;

        let role = create(
            &db,
            "web-server",
            None,
            "MyModule",
            "CentOS",
            "10",
            "arm64",
            &empty_disk_layout(),
            None,
            None,
            None,
        )
        .await
        .unwrap();

        let retrieved = get(&db, role.id.unwrap()).await.unwrap();
        assert_eq!(retrieved.osm_module, "MyModule");
        assert_eq!(retrieved.os_name, "CentOS");
        assert_eq!(retrieved.os_release, "10");
        assert_eq!(retrieved.os_arch, "arm64");
    }

    #[tokio::test]
    async fn test_update_role() {
        let db = setup_db(test_connection_factory!()).await;

        let role = create(
            &db,
            "web-server",
            None,
            "Default",
            "Ubuntu",
            "22.04",
            "x86-64",
            &empty_disk_layout(),
            None,
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
                description: Some(Some("New description")),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        assert_eq!(updated.name, "updated-name");
        assert_eq!(updated.description, Some("New description".to_string()));
        // Unchanged fields should be preserved
        assert_eq!(updated.osm_module, "Default");
        assert_eq!(updated.os_name, "Ubuntu");
        assert_eq!(updated.os_release, "22.04");
        assert_eq!(updated.os_arch, "x86-64");
    }

    #[tokio::test]
    async fn test_update_role_osm_fields() {
        let db = setup_db(test_connection_factory!()).await;

        let role = create(
            &db,
            "web-server",
            None,
            "Default",
            "Ubuntu",
            "22.04",
            "x86-64",
            &empty_disk_layout(),
            None,
            None,
            None,
        )
        .await
        .unwrap();

        let updated = update(
            &db,
            role.id.unwrap(),
            UpdateRoleParams {
                osm_module: Some("Custom"),
                os_name: Some("Debian"),
                os_release: Some("12"),
                os_arch: Some("arm64"),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        assert_eq!(updated.osm_module, "Custom");
        assert_eq!(updated.os_name, "Debian");
        assert_eq!(updated.os_release, "12");
        assert_eq!(updated.os_arch, "arm64");
    }

    #[tokio::test]
    async fn test_create_role_with_cmdline_args() {
        let db = setup_db(test_connection_factory!()).await;

        let role = create(
            &db,
            "web-server",
            None,
            "Default",
            "Ubuntu",
            "24.04",
            "x86-64",
            &empty_disk_layout(),
            Some("console=ttyS0 quiet"),
            None,
            None,
        )
        .await
        .unwrap();

        assert_eq!(role.cmdline_args, Some("console=ttyS0 quiet".to_string()));

        let retrieved = get(&db, role.id.unwrap()).await.unwrap();
        assert_eq!(
            retrieved.cmdline_args,
            Some("console=ttyS0 quiet".to_string())
        );
    }

    #[tokio::test]
    async fn test_delete_role() {
        let db = setup_db(test_connection_factory!()).await;

        let role = create(
            &db,
            "web-server",
            None,
            "Default",
            "Ubuntu",
            "24.04",
            "x86-64",
            &empty_disk_layout(),
            None,
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
        let db = setup_db(test_connection_factory!()).await;

        let role = create(
            &db,
            "uefi-role",
            None,
            "Default",
            "Ubuntu",
            "24.04",
            "x86-64",
            &empty_disk_layout(),
            None,
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
        let db = setup_db(test_connection_factory!()).await;

        let role = create(
            &db,
            "bios-role",
            None,
            "Default",
            "Ubuntu",
            "24.04",
            "x86-64",
            &empty_disk_layout(),
            None,
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
        let db = setup_db(test_connection_factory!()).await;

        let role = create(
            &db,
            "no-firmware-mode-role",
            None,
            "Default",
            "Ubuntu",
            "24.04",
            "x86-64",
            &empty_disk_layout(),
            None,
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
        let db = setup_db(test_connection_factory!()).await;

        let role = create(
            &db,
            "role",
            None,
            "Default",
            "Ubuntu",
            "24.04",
            "x86-64",
            &empty_disk_layout(),
            None,
            None,
            None,
        )
        .await
        .unwrap();

        let updated = update(
            &db,
            role.id.unwrap(),
            UpdateRoleParams {
                firmware_mode: Some(Some(common::FirmwareMode::Uefi)),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        assert_eq!(updated.firmware_mode, Some(common::FirmwareMode::Uefi));

        let retrieved = get(&db, role.id.unwrap()).await.unwrap();
        assert_eq!(retrieved.firmware_mode, Some(common::FirmwareMode::Uefi));
    }

    /// Create a role that has every nullable column populated, so update tests can observe
    /// both "cleared" and "left alone".
    async fn create_populated_role(conn: &Connection) -> i64 {
        create(
            conn,
            "role",
            Some("A description"),
            "Default",
            "Ubuntu",
            "24.04",
            "x86-64",
            &empty_disk_layout(),
            Some("console=ttyS0"),
            Some(&serde_json::json!({"k": "v"})),
            Some(common::FirmwareMode::Uefi),
        )
        .await
        .unwrap()
        .id
        .unwrap()
    }

    #[tokio::test]
    async fn test_update_role_clears_nullable_columns() {
        let db = setup_db(test_connection_factory!()).await;
        let role_id = create_populated_role(&db).await;

        let updated = update(
            &db,
            role_id,
            UpdateRoleParams {
                description: Some(None),
                cmdline_args: Some(None),
                config_template: Some(None),
                firmware_mode: Some(None),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        assert_eq!(updated.description, None);
        assert_eq!(updated.cmdline_args, None);
        assert_eq!(updated.config_template, None);
        assert_eq!(updated.firmware_mode, None);

        let retrieved = get(&db, role_id).await.unwrap();
        assert_eq!(retrieved.description, None);
        assert_eq!(retrieved.cmdline_args, None);
        assert_eq!(retrieved.config_template, None);
        assert_eq!(retrieved.firmware_mode, None);
    }

    #[tokio::test]
    async fn test_update_role_leaves_nullable_columns_alone() {
        let db = setup_db(test_connection_factory!()).await;
        let role_id = create_populated_role(&db).await;

        let updated = update(
            &db,
            role_id,
            UpdateRoleParams {
                name: Some("renamed"),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        assert_eq!(updated.name, "renamed");
        assert_eq!(updated.description, Some("A description".to_string()));
        assert_eq!(updated.cmdline_args, Some("console=ttyS0".to_string()));
        assert_eq!(updated.config_template, Some(serde_json::json!({"k": "v"})));
        assert_eq!(updated.firmware_mode, Some(common::FirmwareMode::Uefi));
    }

    #[tokio::test]
    async fn test_update_role_sets_nullable_columns() {
        let db = setup_db(test_connection_factory!()).await;
        let role_id = create_populated_role(&db).await;

        let new_config = serde_json::json!({"other": 1});
        let updated = update(
            &db,
            role_id,
            UpdateRoleParams {
                description: Some(Some("New description")),
                cmdline_args: Some(Some("quiet")),
                config_template: Some(Some(&new_config)),
                firmware_mode: Some(Some(common::FirmwareMode::Bios)),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        assert_eq!(updated.description, Some("New description".to_string()));
        assert_eq!(updated.cmdline_args, Some("quiet".to_string()));
        assert_eq!(updated.config_template, Some(new_config));
        assert_eq!(updated.firmware_mode, Some(common::FirmwareMode::Bios));
    }
}
