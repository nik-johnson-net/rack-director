use crate::http::{AppState, error::Error};
use crate::templates;
use axum::{
    http::{StatusCode, header},
    response::Response,
};
use common::Ipv4Subnet;
use std::sync::Arc;
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
/// * `state` - Application state containing stores for devices, roles, OS, and images
/// * `device_uuid` - UUID of the device requesting the install script
///
/// # Returns
/// * `Ok(Response)` - HTTP response containing the rendered install script
/// * `Err(Error)` - Error if device not found, missing role, template issues, etc.
pub async fn render_for_device(
    state: &Arc<AppState>,
    device_uuid: &Uuid,
) -> Result<Response<String>, Error> {
    // Get device
    let device = state
        .director
        .get_device(device_uuid)
        .await
        .map_err(Error::ServerInternalError)?;

    // Get device role
    let role = state
        .roles_store
        .get_device_role(device_uuid)
        .await
        .map_err(Error::ServerInternalError)?
        .ok_or_else(|| Error::NotFound("Device has no role assigned".to_string()))?;

    // Get device architecture
    let arch = device.architecture;

    // Get OS architecture configuration
    let os_arch = state
        .os_store
        .get_architecture(role.os_id, arch)
        .await
        .map_err(|e| Error::NotFound(format!("OS architecture not found: {}", e)))?;

    // Get OS
    let os = state
        .os_store
        .get(role.os_id)
        .await
        .map_err(Error::ServerInternalError)?;

    // Check if install script exists
    let script_path = os_arch
        .install_script_path
        .ok_or_else(|| Error::NotFound("No install script for this OS architecture".to_string()))?;

    // Download install script template from storage
    let script_bytes = state
        .image_store
        .download(&script_path)
        .await
        .map_err(|e| {
            Error::ServerInternalError(anyhow::anyhow!("Failed to download script: {}", e))
        })?;

    let template = String::from_utf8(script_bytes).map_err(|e| {
        Error::ServerInternalError(anyhow::anyhow!("Script is not valid UTF-8: {}", e))
    })?;

    // Get device network info
    let network_info = get_device_network_info(state, device_uuid).await?;

    // Get device attributes
    let hostname = device
        .attributes
        .get("hostname")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let device_info = templates::DeviceInfo {
        uuid: *device_uuid,
        hostname,
    };

    // Render template with device context
    let rendered =
        templates::render_install_script(&template, &device_info, &role, &os, &network_info)
            .map_err(|e| {
                Error::ServerInternalError(anyhow::anyhow!("Template rendering failed: {}", e))
            })?;

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/plain")
        .body(rendered)
        .expect("response building should never error"))
}

/// Retrieves network configuration information for a device from its DHCP lease.
///
/// Looks up the device's active DHCP lease and constructs network information
/// including IP address, MAC address, gateway, DNS servers, and netmask.
///
/// # Arguments
/// * `state` - Application state containing DHCP store
/// * `device_uuid` - UUID of the device to get network info for
///
/// # Returns
/// * `Ok(NetworkInfo)` - Network configuration details
/// * `Err(Error::NotFound)` - If device has no active DHCP lease
/// * `Err(Error::ServerInternalError)` - If network lookup fails
pub async fn get_device_network_info(
    state: &Arc<AppState>,
    device_uuid: &Uuid,
) -> Result<templates::NetworkInfo, Error> {
    // Try to find device's lease
    let lease = state
        .dhcp_store
        .find_lease_by_device_uuid(device_uuid)
        .map_err(Error::ServerInternalError)?;

    if let Some(lease) = lease {
        // Get DHCP config for gateway and DNS
        let network = state.dhcp_store.get_network(lease.id).await?;
        let dns_servers = network.dns_servers;

        let subnet: Ipv4Subnet = network.subnet.parse().map_err(anyhow::Error::new)?;

        Ok(templates::NetworkInfo {
            mac_address: lease.mac_address,
            ip_address: lease.ip_address,
            gateway: network.gateway,
            dns_servers,
            netmask: subnet.netmask().to_string(),
        })
    } else {
        Err(Error::NotFound("Device has no DHCP lease".to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database;
    use crate::director::Director;
    use crate::storage::MemoryImageStore;
    use std::net::Ipv4Addr;
    use std::sync::Arc;
    use tempfile::tempdir;
    use tokio::sync::Mutex;
    use uuid::Uuid;

    fn test_uuid() -> Uuid {
        Uuid::parse_str("550e8400-e29b-41d4-a716-446655440001").unwrap()
    }

    async fn create_test_state() -> (Arc<AppState>, tempfile::TempDir) {
        let temp_dir = tempdir().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let db = database::open(&db_path).unwrap();
        let db_tokio = Arc::new(Mutex::new(db));

        let storage_path = temp_dir.path().join("images");
        let image_store = crate::storage::LocalImageStore::new(
            storage_path,
            "http://localhost:8080/images".to_string(),
        )
        .unwrap();

        let agent_images_path = temp_dir.path().join("agent-image");
        std::fs::create_dir_all(&agent_images_path).unwrap();

        // Create boot files directory for testing
        let boot_files_path = temp_dir.path().join("boot");
        std::fs::create_dir_all(&boot_files_path).unwrap();

        let boot_file_provider =
            Arc::new(crate::boot_files::FilesystemBootFileProvider::new(boot_files_path).unwrap());

        let state = Arc::new(AppState {
            director: Director::new(
                db_tokio.clone(),
                Arc::new(MemoryImageStore::new()),
                "http://localhost:8080",
            ),
            dhcp_store: crate::dhcp::DhcpStore::new(db_tokio.clone()),
            image_store: Arc::new(image_store),
            os_store: crate::operating_systems::OperatingSystemsStore::new(db_tokio.clone()),
            roles_store: crate::roles::RolesStore::new(db_tokio),
            agent_images_path,
            boot_file_provider,
        });

        (state, temp_dir)
    }

    #[tokio::test]
    async fn test_get_device_network_info_no_lease() {
        let (state, _temp_dir) = create_test_state().await;

        let uuid = test_uuid();
        state
            .director
            .register_device(&uuid, crate::operating_systems::Architecture::X86_64)
            .await
            .unwrap();

        let result = get_device_network_info(&state, &uuid).await;
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
        let (state, _temp_dir) = create_test_state().await;

        let uuid = test_uuid();
        let mac = "aa:bb:cc:dd:ee:ff";

        state
            .director
            .register_device(&uuid, crate::operating_systems::Architecture::X86_64)
            .await
            .unwrap();

        // Create a DHCP lease
        let ip: Ipv4Addr = "10.0.0.100".parse().unwrap();
        state
            .dhcp_store
            .create_or_update_lease_with_network(
                mac,
                &ip,
                Some(&uuid),
                crate::dhcp::LeaseState::Active,
                3600,
                1,
            )
            .await
            .unwrap();

        let result = get_device_network_info(&state, &uuid).await;
        let network_info = match result {
            Ok(info) => info,
            Err(_) => panic!("Expected Ok, got Err"),
        };
        assert_eq!(network_info.mac_address, mac);
        assert_eq!(network_info.ip_address, "10.0.0.100");
        assert_eq!(network_info.gateway, "10.0.0.1");
        assert_eq!(network_info.netmask, "255.255.255.0");
    }

    #[tokio::test]
    async fn test_render_for_device_no_role() {
        let (state, _temp_dir) = create_test_state().await;

        let uuid = test_uuid();
        state
            .director
            .register_device(&uuid, crate::operating_systems::Architecture::X86_64)
            .await
            .unwrap();

        let result = render_for_device(&state, &uuid).await;
        assert!(result.is_err());
        match result {
            Err(Error::NotFound(msg)) => {
                assert!(msg.contains("no role"));
            }
            _ => panic!("Expected NotFound error for missing role"),
        }
    }
}
