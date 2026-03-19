use crate::http::error::Error;
use crate::templates;
use crate::{database, dhcp, director::Director, operating_systems, roles};
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
/// 3. Downloads the install script template from storage
/// 4. Gathers device network information from DHCP
/// 5. Renders the template with device context
///
/// # Arguments
/// * `conn` - An open database connection
/// * `image_store` - Image store for downloading script templates
/// * `device_uuid` - UUID of the device requesting the install script
///
/// # Returns
/// * `Ok(Response)` - HTTP response containing the rendered install script
/// * `Err(Error)` - Error if device not found, missing role, template issues, etc.
pub async fn render_for_device(
    conn: &database::Connection,
    image_store: &crate::storage::ImageStore,
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

    // Get device architecture
    let arch = device.architecture;

    // Get OS architecture configuration
    let os_arch = operating_systems::store::get_architecture(conn, role.os_id, arch)
        .await
        .map_err(|e| Error::NotFound(format!("OS architecture not found: {}", e)))?;

    // Get OS
    let os = operating_systems::store::get(conn, role.os_id)
        .await
        .map_err(Error::ServerInternalError)?;

    // Check if install script exists
    let script_path = os_arch
        .install_script_path
        .ok_or_else(|| Error::NotFound("No install script for this OS architecture".to_string()))?;

    // Download install script template from storage
    let (stream, _size) = image_store.download(&script_path).await.map_err(|e| {
        Error::ServerInternalError(anyhow::anyhow!("Failed to download script: {}", e))
    })?;

    // Collect stream into bytes (install scripts are small text files)
    let script_bytes = stream
        .try_fold(Vec::new(), |mut vec, data| async move {
            vec.extend_from_slice(&data);
            Ok(vec)
        })
        .await
        .map_err(|e| {
            log::warn!("Error retrieving install script {}: {}", script_path, e);
            Error::ServerInternalError(anyhow!(""))
        })?;

    let template = String::from_utf8(script_bytes).map_err(|e| {
        Error::ServerInternalError(anyhow::anyhow!("Script is not valid UTF-8: {}", e))
    })?;

    // Get device network info
    let network_info = get_device_network_info_with_db(conn, device_uuid).await?;

    // Get device attributes
    let hostname = device.attributes.hostname.clone();

    let device_info = templates::DeviceInfo {
        uuid: *device_uuid,
        hostname,
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
            owned_layout =
                crate::disk_layout::resolve_disk_layout(&role.disk_layout, &platform.attributes)
                    .map_err(Error::ServerInternalError)?;
            &owned_layout
        } else {
            &role.disk_layout
        };

    // Render template with device context
    let rendered = templates::render_install_script(
        &template,
        &device_info,
        &role,
        &os,
        &network_info,
        resolved_disk_layout,
    )
    .map_err(|e| Error::ServerInternalError(anyhow::anyhow!("Template rendering failed: {}", e)))?;

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/plain")
        .body(rendered)
        .expect("response building should never error"))
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
        let network = dhcp::store::get_network(conn, lease.id).await?;
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

        let image_store = ImageStore::memory("http://localhost:8080");

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
            .register_device(&uuid, crate::operating_systems::Architecture::X86_64)
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
            .register_device(&uuid, crate::operating_systems::Architecture::X86_64)
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
                .register_device(&uuid, crate::operating_systems::Architecture::X86_64)
                .await
                .unwrap();
        }

        let conn = state.connection_factory.open().await.unwrap();
        let result = render_for_device(&conn, &state.image_store, &uuid).await;
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
        }
    }
}
