use crate::http::AppState;
use log::warn;
use std::{net::SocketAddr, sync::Arc};
use uuid::Uuid;

/// Resolves the MAC address for a device from query parameter or DHCP lookup.
///
/// Attempts to determine the MAC address in two ways:
/// 1. Directly from the MAC query parameter (if provided and non-empty)
/// 2. By looking up the client IP address in DHCP leases (fallback for older clients)
///
/// # Arguments
/// * `state` - Application state containing DHCP store
/// * `mac_param` - Optional MAC address from query parameter
/// * `client_addr` - Client socket address (IP + port)
///
/// # Returns
/// * `Some(String)` - MAC address if found via parameter or DHCP lookup
/// * `None` - If MAC cannot be determined
pub async fn resolve_mac_address(
    state: &Arc<AppState>,
    mac_param: Option<&String>,
    client_addr: SocketAddr,
) -> Option<String> {
    match mac_param {
        Some(mac) if !mac.is_empty() => Some(mac.clone()),
        _ => {
            // Fallback: Look up MAC address from client IP (may not work in all network setups)
            let client_ip = client_addr.ip().to_string();
            if let Ok(leases) = state.dhcp_store.get_all_leases().await {
                leases
                    .iter()
                    .find(|l| l.ip_address == client_ip)
                    .map(|l| l.mac_address.clone())
            } else {
                None
            }
        }
    }
}

/// Registers a new device and starts the discovery lifecycle.
///
/// This function performs the complete device registration workflow:
/// 1. Checks if a pending device exists for the MAC address (logs if found)
/// 2. Registers the device in the devices table
/// 3. Completes pending device registration (links MAC to UUID)
/// 4. Starts the discovery lifecycle transition (moves device to New state with discovery plan)
///
/// All errors are logged but not propagated, making registration best-effort.
///
/// # Arguments
/// * `state` - Application state containing director
/// * `device_uuid` - UUID of the device to register
/// * `mac_address` - Optional MAC address to link with pending devices
pub async fn register_and_start_discovery(
    state: &Arc<AppState>,
    device_uuid: &Uuid,
    mac_address: Option<&String>,
) {
    // Check for pending device
    if let Some(mac) = mac_address
        && let Ok(Some(_)) = state.director.find_pending_device_by_mac(mac).await
    {
        log::info!(
            "Completing pending device for MAC {} with UUID {}",
            mac,
            device_uuid
        );
    }

    // Register device
    if let Err(e) = state
        .director
        .register_device(device_uuid, crate::operating_systems::Architecture::X86_64)
        .await
    {
        warn!("Couldn't register device {}: {}", device_uuid, e);
        return;
    }

    // Complete pending device link
    if let Some(mac) = mac_address
        && let Err(e) = state
            .director
            .complete_pending_device(mac, device_uuid)
            .await
    {
        warn!("Couldn't complete pending device: {}", e);
    }

    // Create static DHCP reservation from active lease
    if let Some(mac) = mac_address
        && let Ok(Some(lease)) = state.dhcp_store.get_lease_by_mac(mac).await
        && let Some(network_id) = lease.network_id
    {
        let hostname = state
            .director
            .get_device(device_uuid)
            .await
            .ok()
            .and_then(|d| d.attributes.hostname);

        if let Err(e) = state
            .dhcp_store
            .create_or_update_static_reservation(
                network_id,
                mac,
                &lease.ip_address,
                hostname.as_deref(),
            )
            .await
        {
            warn!(
                "Couldn't create static DHCP reservation for device {}: {}",
                device_uuid, e
            );
        }
    }

    // Automatically start discovery transition for newly registered devices
    if let Err(e) = state
        .director
        .start_lifecycle_transition(
            device_uuid,
            crate::lifecycle::DeviceLifecycle::Unprovisioned,
        )
        .await
    {
        warn!(
            "Couldn't start discovery transition for {}: {}",
            device_uuid, e
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database;
    use crate::storage::ImageStore;
    use uuid::Uuid;

    fn test_uuid() -> Uuid {
        Uuid::parse_str("550e8400-e29b-41d4-a716-446655440001").unwrap()
    }
    use crate::director::Director;
    use std::net::Ipv4Addr;
    use std::sync::Arc;
    use tempfile::tempdir;
    use tokio::sync::Mutex;

    async fn create_test_state() -> (Arc<AppState>, tempfile::TempDir) {
        let temp_dir = tempdir().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let db = database::open(&db_path).unwrap();
        let db_tokio = Arc::new(Mutex::new(db));

        // Note: No default network is created. Tests that need networks should create them explicitly.

        let _storage_path = temp_dir.path().join("images");
        let image_store = ImageStore::memory("http://localhost:8080");

        let agent_images_path = temp_dir.path().join("agent-image");
        std::fs::create_dir_all(&agent_images_path).unwrap();

        // Create boot files directory for testing
        let boot_files_path = temp_dir.path().join("boot");
        std::fs::create_dir_all(&boot_files_path).unwrap();

        let boot_file_provider =
            Arc::new(crate::boot_files::FilesystemBootFileProvider::new(boot_files_path).unwrap());

        let state = Arc::new(AppState {
            director: Director::new(db_tokio.clone()),
            dhcp_store: crate::dhcp::DhcpStore::new(db_tokio.clone()),
            image_store: Arc::new(image_store),
            os_store: crate::operating_systems::OperatingSystemsStore::new(db_tokio.clone()),
            roles_store: crate::roles::RolesStore::new(db_tokio.clone()),
            platforms_store: crate::platforms::PlatformsStore::new(db_tokio),
            agent_images_path,
            boot_file_provider,
        });

        (state, temp_dir)
    }

    /// Helper to create a test network for tests that need DHCP functionality
    async fn create_test_network(state: &AppState) -> i64 {
        let network = state
            .dhcp_store
            .create_network(
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

        state
            .dhcp_store
            .create_pool(network.id, "Test Pool", "10.0.0.100", "10.0.0.200")
            .await
            .unwrap();

        network.id
    }

    #[tokio::test]
    async fn test_resolve_mac_address_from_parameter() {
        let (state, _temp_dir) = create_test_state().await;
        let mac = "aa:bb:cc:dd:ee:ff".to_string();
        let addr = "127.0.0.1:1234".parse().unwrap();

        let result = resolve_mac_address(&state, Some(&mac), addr).await;
        assert_eq!(result, Some(mac));
    }

    #[tokio::test]
    async fn test_resolve_mac_address_empty_parameter() {
        let (state, _temp_dir) = create_test_state().await;
        let empty_mac = "".to_string();
        let addr = "127.0.0.1:1234".parse().unwrap();

        let result = resolve_mac_address(&state, Some(&empty_mac), addr).await;
        // Should attempt DHCP lookup, which will return None with no leases
        assert_eq!(result, None);
    }

    #[tokio::test]
    async fn test_resolve_mac_address_from_dhcp() {
        let (state, _temp_dir) = create_test_state().await;
        let network_id = create_test_network(&state).await;
        let mac = "aa:bb:cc:dd:ee:ff";
        let ip: Ipv4Addr = "10.0.0.100".parse().unwrap();

        // Create a DHCP lease
        state
            .dhcp_store
            .create_or_update_lease_with_network(
                mac,
                &ip,
                None,
                crate::dhcp::LeaseState::Active,
                3600,
                network_id,
            )
            .await
            .unwrap();

        let addr: SocketAddr = format!("{}:1234", ip).parse().unwrap();
        let result = resolve_mac_address(&state, None, addr).await;
        assert_eq!(result, Some(mac.to_string()));
    }

    #[tokio::test]
    async fn test_register_and_start_discovery() {
        let (state, _temp_dir) = create_test_state().await;
        let uuid = test_uuid();

        // Verify device doesn't exist
        assert!(!state.director.device_exists(&uuid).await.unwrap());

        register_and_start_discovery(&state, &uuid, None).await;

        // Verify device was registered
        assert!(state.director.device_exists(&uuid).await.unwrap());

        // Verify lifecycle was started (device should be in New state)
        let lifecycle = state.director.get_device_lifecycle(&uuid).await.unwrap();
        assert_eq!(lifecycle, Some(crate::lifecycle::DeviceLifecycle::New));

        // Verify discovery plan was created
        let plan = state
            .director
            .get_active_plan_for_device(&uuid)
            .await
            .unwrap();
        assert!(plan.is_some());
    }

    #[tokio::test]
    async fn test_register_and_start_discovery_with_pending_device() {
        let (state, _temp_dir) = create_test_state().await;
        let network_id = create_test_network(&state).await;
        let uuid = test_uuid();
        let mac = "aa:bb:cc:dd:ee:ff".to_string();

        // Create pending device
        state
            .director
            .create_pending_device(&mac, network_id)
            .await
            .unwrap();

        // Verify pending device exists
        let pending = state
            .director
            .find_pending_device_by_mac(&mac)
            .await
            .unwrap();
        assert!(pending.is_some());

        register_and_start_discovery(&state, &uuid, Some(&mac)).await;

        // Verify device was registered
        assert!(state.director.device_exists(&uuid).await.unwrap());

        // Verify pending device was completed (removed)
        let pending = state
            .director
            .find_pending_device_by_mac(&mac)
            .await
            .unwrap();
        assert!(pending.is_none());
    }

    #[tokio::test]
    async fn test_register_and_start_discovery_creates_static_reservation() {
        let (state, _temp_dir) = create_test_state().await;
        let network_id = create_test_network(&state).await;
        let uuid = test_uuid();
        let mac = "aa:bb:cc:dd:ee:11".to_string();

        // Create a DHCP lease for this MAC (without device_uuid initially)
        use std::net::Ipv4Addr;
        let ip: Ipv4Addr = "10.0.0.150".parse().unwrap();
        state
            .dhcp_store
            .create_or_update_lease_with_network(
                &mac,
                &ip,
                None, // Device doesn't exist yet
                crate::dhcp::LeaseState::Active,
                3600,
                network_id,
            )
            .await
            .unwrap();

        // Register device - this should create a static reservation
        register_and_start_discovery(&state, &uuid, Some(&mac)).await;

        // Verify static reservation was created
        let reservation = state
            .dhcp_store
            .get_static_reservation(network_id, &mac)
            .await
            .unwrap();
        assert!(reservation.is_some());
        let r = reservation.unwrap();
        assert_eq!(r.mac_address, mac);
        assert_eq!(r.ip_address, "10.0.0.150");
        assert_eq!(r.network_id, network_id);

        // Verify hostname is included
        let device = state.director.get_device(&uuid).await.unwrap();
        assert_eq!(r.hostname, device.attributes.hostname);
    }
}
