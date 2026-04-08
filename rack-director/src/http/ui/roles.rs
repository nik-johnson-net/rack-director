use super::super::{AppState, error::Error as HttpError};
use crate::roles::*;
use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{delete, get, post, put},
};
use std::sync::Arc;

pub fn routes(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/ui/roles", post(create_role))
        .route("/ui/roles", get(list_roles))
        .route("/ui/roles/{id}", get(get_role))
        .route("/ui/roles/{id}", put(update_role))
        .route("/ui/roles/{id}", delete(delete_role))
        .route("/ui/roles/{id}/devices", get(list_role_devices))
        .with_state(state)
}

// Create a new role
async fn create_role(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateRoleRequest>,
) -> Result<(StatusCode, Json<Role>), HttpError> {
    // Validate the disk layout before persisting.
    if let Err(errors) = crate::disk_layout::validate_disk_layout(&req.disk_layout) {
        return Err(HttpError::ValidationError(errors));
    }

    let conn = state.connection_factory.open().await?;

    // Verify the referenced OS exists in the OSM registry
    crate::osm::resolve_os(
        &conn,
        &req.osm_module,
        &req.os_name,
        &req.os_release,
        &req.os_arch,
    )
    .await
    .map_err(|e| HttpError::BadRequest(format!("Invalid OS reference: {}", e)))?;

    let role = crate::roles::store::create(
        &conn,
        &req.name,
        req.description.as_deref(),
        &req.osm_module,
        &req.os_name,
        &req.os_release,
        &req.os_arch,
        &req.disk_layout,
        req.cmdline_args.as_deref(),
        req.config_template.as_ref(),
        req.firmware_mode,
    )
    .await?;

    Ok((StatusCode::CREATED, Json(role)))
}

// List all roles
async fn list_roles(State(state): State<Arc<AppState>>) -> Result<Json<Vec<Role>>, HttpError> {
    let conn = state.connection_factory.open().await?;
    let roles = crate::roles::store::list(&conn).await?;
    Ok(Json(roles))
}

// Get a specific role
async fn get_role(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<Role>, HttpError> {
    let conn = state.connection_factory.open().await?;
    let role = crate::roles::store::get(&conn, id).await?;
    Ok(Json(role))
}

// Update a role
#[axum::debug_handler]
async fn update_role(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(req): Json<UpdateRoleRequest>,
) -> Result<Json<Role>, HttpError> {
    // Validate the new disk layout before applying any changes.
    if let Some(ref disk_layout) = req.disk_layout
        && let Err(errors) = crate::disk_layout::validate_disk_layout(disk_layout)
    {
        return Err(HttpError::ValidationError(errors));
    }

    let conn = state.connection_factory.open().await?;

    // If updating any OSM fields, validate the resulting OS reference
    if req.osm_module.is_some()
        || req.os_name.is_some()
        || req.os_release.is_some()
        || req.os_arch.is_some()
    {
        // Get current role to fill in unchanged fields
        let current = crate::roles::store::get(&conn, id).await?;
        let module = req.osm_module.as_deref().unwrap_or(&current.osm_module);
        let name = req.os_name.as_deref().unwrap_or(&current.os_name);
        let release = req.os_release.as_deref().unwrap_or(&current.os_release);
        let arch = req.os_arch.as_deref().unwrap_or(&current.os_arch);
        crate::osm::resolve_os(&conn, module, name, release, arch)
            .await
            .map_err(|e| HttpError::BadRequest(format!("Invalid OS reference: {}", e)))?;
    }

    // Platform compatibility check: if the new disk layout uses labels, verify that every
    // device currently assigned to this role has a platform that satisfies all required labels.
    if let Some(ref new_layout) = req.disk_layout
        && crate::disk_layout::layout_uses_labels(new_layout)
    {
        check_platform_compatibility(&conn, id, new_layout).await?;
    }

    let role = crate::roles::store::update(
        &conn,
        id,
        crate::roles::store::UpdateRoleParams {
            name: req.name.as_deref(),
            description: req.description.as_deref(),
            osm_module: req.osm_module.as_deref(),
            os_name: req.os_name.as_deref(),
            os_release: req.os_release.as_deref(),
            os_arch: req.os_arch.as_deref(),
            disk_layout: req.disk_layout.as_ref(),
            cmdline_args: req.cmdline_args.as_deref(),
            config_template: req.config_template.as_ref(),
            firmware_mode: req.firmware_mode,
            clear_firmware_mode: req.clear_firmware_mode,
        },
    )
    .await?;

    Ok(Json(role))
}

/// Check that every device assigned to `role_id` has a platform that satisfies all disk
/// labels in `new_layout`.
///
/// Returns an `HttpError::ValidationError` if any device's platform cannot resolve all labels,
/// so the caller receives a structured 400 response instead of an internal server error.
///
/// Devices without a platform are skipped — label resolution for those will fail at
/// provisioning time, which is handled elsewhere.
async fn check_platform_compatibility(
    conn: &crate::database::Connection,
    role_id: i64,
    new_layout: &crate::roles::DiskLayout,
) -> Result<(), HttpError> {
    let device_uuids = crate::director::store::list_devices_with_role(conn, role_id).await?;

    // Collect distinct platform IDs without loading full device rows.
    let mut platform_ids = std::collections::HashSet::new();
    for uuid in &device_uuids {
        if let Some(pid) = crate::director::store::get_device_platform_id(conn, uuid).await? {
            platform_ids.insert(pid);
        }
    }

    // Validate each distinct platform once, avoiding redundant queries when many devices
    // share the same platform.
    for platform_id in &platform_ids {
        let platform = crate::platforms::store::get(conn, *platform_id).await?;

        if let Err(e) =
            crate::disk_layout::validate_layout_against_platform(new_layout, &platform.attributes)
        {
            let mut errors = std::collections::HashMap::new();
            errors.insert(
                "disk_layout".to_string(),
                format!(
                    "Platform '{}' does not have all required disk labels: {}. \
                     Remove the role from affected devices first, or update the platform.",
                    platform.name, e
                ),
            );
            return Err(HttpError::ValidationError(errors));
        }
    }

    Ok(())
}

// Delete a role
async fn delete_role(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<StatusCode, HttpError> {
    let conn = state.connection_factory.open().await?;
    crate::roles::store::delete(&conn, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::operating_systems::Architecture;
    use crate::platforms::{DiskType, PlatformAttributes, PlatformCpu, PlatformDisk, PlatformNic};
    use crate::{database, test_connection_factory};
    use common::disk_layout::{DiskConfig, DiskLayout, PartitionConfig};
    use uuid::Uuid;

    /// Set up a migrated in-memory database, returning a connection that keeps the
    /// in-memory SQLite alive for the duration of the test.
    async fn setup_db(factory: database::DatabaseConnectionFactory) -> database::Connection {
        database::run_migrations(&factory).await.unwrap()
    }

    /// A deterministic UUID helper so tests produce readable failure messages.
    fn test_uuid(n: u8) -> Uuid {
        Uuid::parse_str(&format!("00000000-0000-0000-0000-0000000000{:02x}", n)).unwrap()
    }

    /// A label-based disk layout that references the "ROOT" platform label.
    fn label_layout() -> DiskLayout {
        DiskLayout {
            disks: vec![DiskConfig {
                device: "ROOT".to_string(),
                partition_table: "gpt".to_string(),
                partitions: vec![PartitionConfig {
                    label: "root".to_string(),
                    size: "rest".to_string(),
                    filesystem: Some("ext4".to_string()),
                    mount_point: Some("/".to_string()),
                    flags: None,
                    volume_group: None,
                }],
            }],
            volume_groups: None,
            zfs_pools: None,
        }
    }

    /// A label-based layout that references a label ("DATA1") that our test platform
    /// does NOT define — used to trigger the compatibility failure path.
    fn incompatible_label_layout() -> DiskLayout {
        DiskLayout {
            disks: vec![DiskConfig {
                device: "DATA1".to_string(),
                partition_table: "gpt".to_string(),
                partitions: vec![PartitionConfig {
                    label: "data".to_string(),
                    size: "rest".to_string(),
                    filesystem: Some("xfs".to_string()),
                    mount_point: Some("/data".to_string()),
                    flags: None,
                    volume_group: None,
                }],
            }],
            volume_groups: None,
            zfs_pools: None,
        }
    }

    /// Platform attributes that only declare a "ROOT" disk label — no "DATA1".
    fn root_only_platform_attrs() -> PlatformAttributes {
        PlatformAttributes {
            disks: vec![PlatformDisk {
                size_gb: 480,
                disk_type: DiskType::Ssd,
                label: Some("ROOT".to_string()),
            }],
            nics: vec![PlatformNic {
                logical: "eno1".to_string(),
                speed_mbps: Some(1000),
                label: Some("NIC1".to_string()),
            }],
            cpus: vec![PlatformCpu {
                brand: "intel".to_string(),
                model: "E3-1240 v3".to_string(),
                cores: 4,
            }],
            memory_gib: 16,
        }
    }

    /// Create an OSM module with a single Ubuntu 24.04 x86-64 OS entry.
    async fn create_osm_module(conn: &database::Connection) {
        crate::osm::store::create_module(
            conn,
            "Default",
            "1.0.0",
            "Test",
            "Test module",
            "bundled",
            "osm/Default/1.0.0/",
            None,
        )
        .await
        .unwrap();
        let config = osm::os_config::OperatingSystemConfig {
            name: "Ubuntu".to_string(),
            release: "24.04".to_string(),
            architectures: vec![osm::os_config::ArchitectureConfig {
                arch: "x86-64".to_string(),
                kernel: "vmlinuz".to_string(),
                initramfs: "initrd.img".to_string(),
                modules: vec![],
                cmdline: String::new(),
                install_template: "install.sh".to_string(),
            }],
            template_variables: vec![],
        };
        crate::osm::store::create_operating_system(conn, 1, "ubuntu", "Ubuntu", "24.04", &config)
            .await
            .unwrap();
    }

    /// Create a role with the given disk layout, returning its ID.
    async fn create_role_helper(conn: &database::Connection, layout: &DiskLayout) -> i64 {
        crate::roles::store::create(
            conn,
            "test-role",
            None,
            "Default",
            "Ubuntu",
            "24.04",
            "x86-64",
            layout,
            None,
            None,
            None,
        )
        .await
        .unwrap()
        .id
        .unwrap()
    }

    /// Register a device and assign it to a role.
    async fn create_device_with_role(conn: &database::Connection, uuid: Uuid, role_id: i64) {
        crate::director::store::register_device(conn, &uuid, Architecture::X86_64)
            .await
            .unwrap();
        crate::director::store::assign_role_to_device(conn, &uuid, role_id)
            .await
            .unwrap();
    }

    // ---------------------------------------------------------------------------
    // Tests for check_platform_compatibility
    // ---------------------------------------------------------------------------

    /// When no devices are assigned to the role, compatibility always passes regardless
    /// of the disk layout — there is nothing to validate against.
    #[tokio::test]
    async fn test_no_devices_returns_ok() {
        let conn = setup_db(test_connection_factory!()).await;
        create_osm_module(&conn).await;
        let role_id = create_role_helper(&conn, &label_layout()).await;

        let result = check_platform_compatibility(&conn, role_id, &label_layout()).await;
        assert!(result.is_ok());
    }

    /// When assigned devices have no platform, they are skipped and the check passes.
    ///
    /// Devices without a platform will fail at provisioning time, which is handled
    /// by the provisioning pipeline — the role update must not be blocked here.
    #[tokio::test]
    async fn test_devices_without_platform_returns_ok() {
        let conn = setup_db(test_connection_factory!()).await;
        create_osm_module(&conn).await;
        let role_id = create_role_helper(&conn, &label_layout()).await;
        let device_uuid = test_uuid(1);
        create_device_with_role(&conn, device_uuid, role_id).await;
        // Device has no platform assigned — platform_id remains NULL.

        let result = check_platform_compatibility(&conn, role_id, &label_layout()).await;
        assert!(result.is_ok());
    }

    /// When assigned devices have a platform that defines all labels required by the
    /// layout, the check passes.
    #[tokio::test]
    async fn test_compatible_platform_returns_ok() {
        let conn = setup_db(test_connection_factory!()).await;
        create_osm_module(&conn).await;
        let role_id = create_role_helper(&conn, &label_layout()).await;
        let device_uuid = test_uuid(1);
        create_device_with_role(&conn, device_uuid, role_id).await;

        // Create a platform that has the "ROOT" label required by label_layout().
        let platform = crate::platforms::store::create(
            &conn,
            "Test Platform",
            None,
            &root_only_platform_attrs(),
            None,
        )
        .await
        .unwrap();
        let platform_id = platform.id.unwrap();

        crate::director::store::assign_platform_to_device(&conn, &device_uuid, platform_id)
            .await
            .unwrap();

        let result = check_platform_compatibility(&conn, role_id, &label_layout()).await;
        assert!(result.is_ok());
    }

    /// When an assigned device has a platform that is missing a label referenced by
    /// the layout, the check returns a ValidationError with the "disk_layout" key and
    /// the platform name in the error message.
    #[tokio::test]
    async fn test_incompatible_platform_returns_validation_error() {
        let conn = setup_db(test_connection_factory!()).await;
        create_osm_module(&conn).await;
        // The role itself uses a label-based layout — content doesn't affect this check.
        let role_id = create_role_helper(&conn, &label_layout()).await;
        let device_uuid = test_uuid(1);
        create_device_with_role(&conn, device_uuid, role_id).await;

        // Platform only has "ROOT" — the new layout asks for "DATA1" which is absent.
        let platform = crate::platforms::store::create(
            &conn,
            "Sparse Platform",
            None,
            &root_only_platform_attrs(),
            None,
        )
        .await
        .unwrap();
        let platform_id = platform.id.unwrap();

        crate::director::store::assign_platform_to_device(&conn, &device_uuid, platform_id)
            .await
            .unwrap();

        let result =
            check_platform_compatibility(&conn, role_id, &incompatible_label_layout()).await;

        match result {
            Err(HttpError::ValidationError(errors)) => {
                assert!(
                    errors.contains_key("disk_layout"),
                    "expected 'disk_layout' error key, got: {:?}",
                    errors
                );
                let msg = &errors["disk_layout"];
                assert!(
                    msg.contains("Sparse Platform"),
                    "error should mention the platform name, got: {}",
                    msg
                );
                assert!(
                    msg.contains("DATA1"),
                    "error should mention the missing label, got: {}",
                    msg
                );
            }
            Ok(_) => panic!("expected ValidationError, but got Ok"),
            Err(_) => panic!("expected ValidationError variant, got a different HttpError variant"),
        }
    }

    /// When two devices share the same platform, the platform is validated only once.
    /// This test also verifies that duplicate platform IDs don't cause spurious errors.
    #[tokio::test]
    async fn test_multiple_devices_same_platform_returns_ok() {
        let conn = setup_db(test_connection_factory!()).await;
        create_osm_module(&conn).await;
        let role_id = create_role_helper(&conn, &label_layout()).await;

        let platform = crate::platforms::store::create(
            &conn,
            "Shared Platform",
            None,
            &root_only_platform_attrs(),
            None,
        )
        .await
        .unwrap();
        let platform_id = platform.id.unwrap();

        for n in 1..=3u8 {
            let uuid = test_uuid(n);
            create_device_with_role(&conn, uuid, role_id).await;
            crate::director::store::assign_platform_to_device(&conn, &uuid, platform_id)
                .await
                .unwrap();
        }

        let result = check_platform_compatibility(&conn, role_id, &label_layout()).await;
        assert!(result.is_ok());
    }
}

// List all devices with a specific role
async fn list_role_devices(
    State(state): State<Arc<AppState>>,
    Path(role_id): Path<i64>,
) -> Result<Json<Vec<String>>, HttpError> {
    let conn = state.connection_factory.open().await?;
    let devices = crate::director::store::list_devices_with_role(&conn, role_id).await?;
    let device_strs: Vec<String> = devices.iter().map(|u| u.to_string()).collect();
    Ok(Json(device_strs))
}
