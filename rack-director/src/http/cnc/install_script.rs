use crate::http::error::Error;
use crate::templates;
use crate::{database, dhcp, director::Director, roles};
use anyhow::anyhow;
use axum::{
    http::{StatusCode, header},
    response::Response,
};
use common::Ipv4Subnet;
use futures::TryStreamExt;
use uuid::Uuid;

/// Renders an install script template for a specific device.
///
/// This function orchestrates the complete install script rendering process:
/// 1. Retrieves device information and role assignment
/// 2. Fetches OS and architecture configuration
/// 3. Loads the install script template — from disk for bundled OSMs, or from
///    the image store for uploaded OSMs
/// 4. Gathers device network information from DHCP
/// 5. Renders the template with device context
///
/// # Arguments
/// * `conn` - An open database connection
/// * `image_store` - Image store for downloading script templates (uploaded OSMs)
/// * `bundled_osm_path` - Root directory for bundled OSM files on disk, if configured
/// * `device_uuid` - UUID of the device requesting the install script
/// * `root_url` - Base URL used in rendered template variables
///
/// # Returns
/// * `Ok(Response)` - HTTP response containing the rendered install script
/// * `Err(Error)` - Error if device not found, missing role, template issues, etc.
pub async fn render_for_device(
    conn: &database::Connection,
    image_store: &crate::storage::ImageStore,
    bundled_osm_path: Option<&std::path::Path>,
    device_uuid: &Uuid,
) -> Result<Response<String>, Error> {
    let director = Director::new(conn);

    // Get device
    let device = director
        .get_device(device_uuid)
        .await
        .map_err(Error::ServerInternalError)?;

    // Get device role
    let role_id = crate::director::store::get_device_role_id(conn, device_uuid)
        .await
        .map_err(Error::ServerInternalError)?
        .ok_or_else(|| Error::NotFound("Device has no role assigned".to_string()))?;

    let role = roles::store::get(conn, role_id)
        .await
        .map_err(Error::ServerInternalError)?;

    // Resolve OS from OSM
    let resolved = crate::osm::resolve_os(
        conn,
        &role.osm_module,
        &role.os_name,
        &role.os_release,
        &role.os_arch,
    )
    .await
    .map_err(|e| Error::ServerInternalError(anyhow::anyhow!("Failed to resolve OS: {}", e)))?;

    // Load install script template — bundled modules are served from disk,
    // uploaded modules from the image store.
    let template = if resolved.module.source == "bundled" {
        load_bundled_template(bundled_osm_path, &resolved).await?
    } else {
        load_stored_template(image_store, &resolved).await?
    };

    // Get device network info
    let network_info = get_device_network_info_with_db(conn, device_uuid).await?;

    // Get device attributes
    let hostname = device.attributes.hostname.clone();
    let boot_mode = device.attributes.boot_mode;

    let device_info = templates::DeviceInfo {
        uuid: *device_uuid,
        hostname,
        boot_mode,
    };

    // Get resolved disk layout for template context.
    // If the layout uses platform labels, resolve them to actual device paths
    // using the device's assigned platform attributes.
    let owned_layout;
    let resolved_disk_layout: &common::disk_layout::DiskLayout =
        if crate::disk_layout::layout_uses_labels(&role.disk_layout) {
            let platform_id = device.platform_id.ok_or_else(|| {
                Error::BadRequest(
                    "Disk layout uses platform labels but device has no platform assigned"
                        .to_string(),
                )
            })?;
            let platform = crate::platforms::store::get(conn, platform_id)
                .await
                .map_err(Error::ServerInternalError)?;
            owned_layout = crate::disk_layout::resolve_disk_layout(
                &role.disk_layout,
                &platform.attributes,
                &device.attributes,
            )
            .map_err(Error::ServerInternalError)?;
            &owned_layout
        } else {
            &role.disk_layout
        };

    // Render with OSM function
    let rendered = templates::render_install_script_osm(
        &template,
        &device_info,
        &role.name,
        &role.disk_layout,
        &role.config_template,
        &role.os_name,
        &role.os_release,
        &network_info,
        resolved_disk_layout,
        "http://localhost",
    )
    .map_err(|e| Error::ServerInternalError(anyhow::anyhow!("Template render failed: {}", e)))?;

    log::debug!("Rendered install script for {}:\n{}", device_uuid, rendered);

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/plain")
        .body(rendered)
        .expect("response building should never error"))
}

/// Load an install script template from the on-disk bundled OSM directory.
///
/// Bundled OSMs are never uploaded to the image store — their files live only
/// on disk at `{bundled_osm_path}/{os_dir}/{install_template}`.
async fn load_bundled_template(
    bundled_osm_path: Option<&std::path::Path>,
    resolved: &crate::osm::ResolvedOs,
) -> Result<String, Error> {
    let path = bundled_osm_path.ok_or_else(|| {
        Error::ServerInternalError(anyhow::anyhow!(
            "Bundled OSM '{}' requires --bundled-osm-path to be set",
            resolved.module.name
        ))
    })?;
    let disk_path = path
        .join(&resolved.os.dir_name)
        .join(&resolved.arch_config.install_template);

    // Guard against path traversal: `dir_name` and `install_template` come from
    // the database and could contain `..` components if a corrupt or malicious OSM
    // archive was accepted. Reject any path that escapes the base directory.
    if !path_is_within_base(path, &disk_path) {
        return Err(Error::ServerInternalError(anyhow::anyhow!(
            "Install template path '{}' is outside the bundled OSM directory",
            disk_path.display()
        )));
    }

    let bytes = tokio::fs::read(&disk_path).await.map_err(|e| {
        Error::ServerInternalError(anyhow::anyhow!(
            "Failed to read bundled install template {}: {}",
            disk_path.display(),
            e
        ))
    })?;
    String::from_utf8(bytes).map_err(|e| {
        Error::ServerInternalError(anyhow::anyhow!("Template is not valid UTF-8: {}", e))
    })
}

/// Returns `true` if `path` is lexically within `base` after resolving all `..` components.
///
/// This is a defence-in-depth check — it catches `..`-based traversals without requiring
/// filesystem access (no symlink resolution).  A separate `canonicalize` check would be
/// needed to block symlink-based traversals, but those are not a realistic threat for
/// bundled OSM files shipped with the binary.
fn path_is_within_base(base: &std::path::Path, path: &std::path::Path) -> bool {
    use std::path::Component;
    let mut normalized = std::path::PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                normalized.pop();
            }
            c => normalized.push(c),
        }
    }
    normalized.starts_with(base)
}

/// Load an install script template from the image store (uploaded OSMs).
async fn load_stored_template(
    image_store: &crate::storage::ImageStore,
    resolved: &crate::osm::ResolvedOs,
) -> Result<String, Error> {
    let template_path = resolved.install_template_storage_path();
    let (stream, _size) = image_store.download(&template_path).await.map_err(|e| {
        Error::ServerInternalError(anyhow::anyhow!("Failed to download template: {}", e))
    })?;
    let script_bytes = stream
        .try_fold(Vec::new(), |mut vec, data| async move {
            vec.extend_from_slice(&data);
            Ok(vec)
        })
        .await
        .map_err(|e| {
            log::warn!("Error retrieving install script {}: {}", template_path, e);
            Error::ServerInternalError(anyhow!(""))
        })?;
    String::from_utf8(script_bytes).map_err(|e| {
        Error::ServerInternalError(anyhow::anyhow!("Script is not valid UTF-8: {}", e))
    })
}

/// Inner implementation that accepts an already-opened database connection.
async fn get_device_network_info_with_db(
    conn: &database::Connection,
    device_uuid: &Uuid,
) -> Result<templates::NetworkInfo, Error> {
    // Try to find device's lease
    let lease = dhcp::store::find_lease_by_device_uuid(conn, device_uuid)
        .await
        .map_err(Error::ServerInternalError)?;

    if let Some(lease) = lease {
        // Get DHCP config for gateway and DNS
        let network_id = lease.network_id.ok_or_else(|| {
            Error::ServerInternalError(anyhow::anyhow!(
                "Lease {} has no associated network",
                lease.id
            ))
        })?;
        let network = dhcp::store::get_network(conn, network_id).await?;
        let dns_servers = network.dns_servers;

        let subnet: Ipv4Subnet = network.subnet.parse().map_err(anyhow::Error::new)?;

        Ok(templates::NetworkInfo {
            mac_address: lease.mac_address,
            ip_address: lease.ip_address,
            gateway: network.gateway,
            dns_servers,
            netmask: subnet.netmask().to_string(),
            prefix_length: subnet.subnet(),
        })
    } else {
        Err(Error::NotFound("Device has no DHCP lease".to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::{DatabaseConnectionFactory, run_migrations};

    // ── path_is_within_base ────────────────────────────────────────────────────

    #[test]
    fn test_path_within_base_safe_path() {
        let base = std::path::Path::new("/bundled");
        let path = base.join("almalinux-10").join("install.ks");
        assert!(path_is_within_base(base, &path));
    }

    #[test]
    fn test_path_within_base_dotdot_traversal_rejected() {
        let base = std::path::Path::new("/bundled");
        let path = base.join("../escape").join("install.ks");
        assert!(!path_is_within_base(base, &path));
    }

    #[test]
    fn test_path_within_base_deeply_nested_traversal_rejected() {
        let base = std::path::Path::new("/bundled");
        let path = base.join("subdir/../../etc/passwd");
        assert!(!path_is_within_base(base, &path));
    }

    #[test]
    fn test_path_within_base_exact_base_ok() {
        let base = std::path::Path::new("/bundled");
        assert!(path_is_within_base(base, base));
    }
    use crate::http::AppState;
    use crate::storage::ImageStore;
    use crate::test_database_path;
    use std::net::Ipv4Addr;
    use std::sync::Arc;
    use tempfile::tempdir;
    use uuid::Uuid;

    fn test_uuid() -> Uuid {
        Uuid::parse_str("550e8400-e29b-41d4-a716-446655440001").unwrap()
    }

    async fn create_test_conn(path: String) -> database::Connection {
        let factory = DatabaseConnectionFactory::new(std::path::PathBuf::from(path));
        run_migrations(&factory).await.unwrap()
    }

    async fn create_test_state(
        factory: DatabaseConnectionFactory,
    ) -> (Arc<AppState>, tempfile::TempDir, database::Connection) {
        let conn: Arc<dyn crate::database::ConnectionFactory> = Arc::new(factory);
        // Run migrations and retain the connection so the in-memory DB persists for the test
        let migration_conn = run_migrations(conn.as_ref()).await.unwrap();

        let image_store = ImageStore::memory();

        // Create temporary directories needed for agent images and boot files
        let temp_dir = tempdir().unwrap();

        let agent_images_path = temp_dir.path().join("agent-image");
        std::fs::create_dir_all(&agent_images_path).unwrap();

        // Create boot files directory for testing
        let boot_files_path = temp_dir.path().join("boot");
        std::fs::create_dir_all(&boot_files_path).unwrap();

        let boot_file_provider =
            Arc::new(crate::boot_files::FilesystemBootFileProvider::new(boot_files_path).unwrap());

        let state = Arc::new(AppState {
            connection_factory: conn,
            image_store: Arc::new(image_store),
            agent_images_path,
            boot_file_provider,
            dhcp: crate::dhcp::DhcpControl::noop(),
            unprovisioned_sleep_secs: 600,
            bundled_osm_path: None,
        });

        (state, temp_dir, migration_conn)
    }

    /// Helper to create a test network for tests that need DHCP functionality
    async fn create_test_network(conn: &database::Connection) -> i64 {
        let network = crate::dhcp::store::create_network(
            conn,
            "Test Network",
            "10.0.0.0/24",
            "10.0.0.1",
            &["8.8.8.8".to_string()],
            86400,
            None,
            false,
        )
        .await
        .unwrap();

        crate::dhcp::store::create_pool(conn, network.id, "Test Pool", "10.0.0.100", "10.0.0.200")
            .await
            .unwrap();

        network.id
    }

    #[tokio::test]
    async fn test_get_device_network_info_no_lease() {
        let conn = create_test_conn(test_database_path!()).await;

        let uuid = test_uuid();
        Director::new(&conn)
            .register_device(&uuid, crate::director::Architecture::X86_64)
            .await
            .unwrap();

        let result = get_device_network_info_with_db(&conn, &uuid).await;
        assert!(result.is_err());
        match result {
            Err(Error::NotFound(msg)) => {
                assert!(msg.contains("no DHCP lease"));
            }
            _ => panic!("Expected NotFound error"),
        }
    }

    #[tokio::test]
    async fn test_get_device_network_info_with_lease() {
        let conn = create_test_conn(test_database_path!()).await;
        let network_id = create_test_network(&conn).await;

        let uuid = test_uuid();
        let mac = "aa:bb:cc:dd:ee:ff";

        Director::new(&conn)
            .register_device(&uuid, crate::director::Architecture::X86_64)
            .await
            .unwrap();

        // Create a DHCP lease
        let ip: Ipv4Addr = "10.0.0.100".parse().unwrap();
        crate::dhcp::store::create_or_update_lease_with_network(
            &conn,
            mac,
            &ip,
            Some(&uuid),
            crate::dhcp::LeaseState::Active,
            3600,
            network_id,
        )
        .await
        .unwrap();

        let result = get_device_network_info_with_db(&conn, &uuid).await;
        let network_info = match result {
            Ok(info) => info,
            Err(_) => panic!("Expected Ok, got Err"),
        };
        assert_eq!(network_info.mac_address, mac);
        assert_eq!(network_info.ip_address, "10.0.0.100");
        assert_eq!(network_info.gateway, "10.0.0.1");
        assert_eq!(network_info.netmask, "255.255.255.0");
        assert_eq!(network_info.prefix_length, 24);
    }

    #[tokio::test]
    async fn test_render_for_device_no_role() {
        use crate::test_connection_factory;
        let (state, _temp_dir, _migration_conn) =
            create_test_state(test_connection_factory!()).await;

        let uuid = test_uuid();
        {
            let conn = state.connection_factory.open().await.unwrap();
            Director::new(&conn)
                .register_device(&uuid, crate::director::Architecture::X86_64)
                .await
                .unwrap();
        }

        let conn = state.connection_factory.open().await.unwrap();
        let result = render_for_device(&conn, &state.image_store, None, &uuid).await;
        // Should get NotFound error when device has no role
        match result {
            Ok(_) => panic!("Expected error but got Ok"),
            Err(Error::NotFound(_)) => {
                // Success - this is the expected error
            }
            Err(Error::ServerInternalError(e)) => {
                panic!("Expected NotFound but got ServerInternalError: {}", e)
            }
            Err(Error::ValidationError(_)) => panic!("Expected NotFound but got ValidationError"),
            Err(Error::BadRequest(_)) => panic!("Expected NotFound but got BadRequest"),
            Err(Error::UnprocessableEntity(_)) => {
                panic!("Expected NotFound but got UnprocessableEntity")
            }
        }
    }
}
