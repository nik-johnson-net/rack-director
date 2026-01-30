use crate::director::NetworkInterface;
use crate::http::AppState;
use log::warn;
use std::sync::Arc;
use uuid::Uuid;

/// Enriches network interfaces with IP addresses and network IDs from DHCP leases.
///
/// Takes a list of network interfaces (typically from device hardware discovery) and
/// backfills IP addresses and network IDs by looking up DHCP leases for each MAC address.
/// This allows us to associate discovered hardware interfaces with their network assignments.
///
/// # Arguments
/// * `state` - Application state containing DHCP store
/// * `interfaces` - Vector of NetworkInterface objects to enrich
///
/// # Returns
/// A new vector of NetworkInterface objects with IP addresses and network IDs populated
/// from DHCP lease data where available.
pub async fn enrich_interfaces_with_dhcp_info(
    state: &Arc<AppState>,
    interfaces: Vec<NetworkInterface>,
) -> Vec<NetworkInterface> {
    let mut enriched = Vec::new();

    for mut nic in interfaces {
        // Look up DHCP lease for this MAC
        if let Ok(Some(lease)) = state.dhcp_store.get_lease_by_mac(&nic.mac_address).await {
            log::info!(
                "Backfilling IP {} and network_id {} for NIC {} (MAC {})",
                lease.ip_address,
                lease.network_id.unwrap_or(-1),
                nic.interface_name,
                nic.mac_address
            );
            nic.ip_address = Some(lease.ip_address);
            nic.network_id = lease.network_id;
        }
        enriched.push(nic);
    }

    enriched
}

/// Detects and marks duplicate MAC addresses on the same network.
///
/// For each interface with a network_id, checks if the MAC address exists on other devices
/// within the same network. Duplicate MACs are disabled and marked with a warning label
/// to prevent network conflicts.
///
/// # Arguments
/// * `state` - Application state containing director and DHCP store
/// * `device_uuid` - UUID of the device being checked
/// * `interfaces` - Mutable reference to interfaces to check and potentially mark as disabled
///
/// # Side Effects
/// Modifies interfaces in-place, setting `disabled = true` and `warning_label = Some(...)`
/// for any interface with a duplicate MAC on the same network.
pub async fn detect_and_mark_duplicates(
    state: &Arc<AppState>,
    device_uuid: &Uuid,
    interfaces: &mut [NetworkInterface],
) {
    for nic in interfaces.iter_mut() {
        // Only check for duplicates if the interface has a network_id
        if let Some(network_id) = nic.network_id {
            match state
                .director
                .find_duplicate_macs_on_network(&nic.mac_address, network_id, device_uuid)
                .await
            {
                Ok(duplicates) if !duplicates.is_empty() => {
                    // Get network name for warning message
                    let network_name = match state.dhcp_store.get_network(network_id).await {
                        Ok(network) => network.name,
                        Err(_) => format!("network {}", network_id),
                    };

                    nic.disabled = true;
                    nic.warning_label = Some(format!("Duplicate MAC on {}", network_name));

                    // Log warning with all duplicate devices
                    let duplicate_list: Vec<String> = duplicates
                        .iter()
                        .map(|(dev_uuid, iface)| format!("{}:{}", dev_uuid, iface))
                        .collect();
                    log::warn!(
                        "Duplicate MAC {} detected on network '{}' for device {} interface {}. \
                         Also found on: {}",
                        nic.mac_address,
                        network_name,
                        device_uuid,
                        nic.interface_name,
                        duplicate_list.join(", ")
                    );
                }
                Ok(_) => {
                    // No duplicates - ensure interface is not disabled
                    nic.disabled = false;
                    nic.warning_label = None;
                }
                Err(e) => {
                    warn!(
                        "Error checking for duplicate MAC {} on network {}: {}",
                        nic.mac_address, network_id, e
                    );
                }
            }
        }
    }
}

/// Completes pending device registrations for discovered interfaces.
///
/// When a device reports its network interfaces, checks if any of those MAC addresses
/// match pending devices (devices that were created via DHCP but haven't completed
/// registration yet). If a match is found, links the pending device to this device UUID.
///
/// # Arguments
/// * `state` - Application state containing director
/// * `device_uuid` - UUID of the device that owns these interfaces
/// * `interfaces` - Network interfaces to check against pending devices
///
/// # Side Effects
/// Completes pending device registration for any matching MAC addresses, linking them
/// to the provided device UUID.
pub async fn complete_pending_devices_for_interfaces(
    state: &Arc<AppState>,
    device_uuid: &Uuid,
    interfaces: &[NetworkInterface],
) {
    for nic in interfaces {
        if let Err(e) = state
            .director
            .complete_pending_device(&nic.mac_address, device_uuid)
            .await
        {
            log::debug!(
                "Could not complete pending device for MAC {}: {}",
                nic.mac_address,
                e
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database;
    use uuid::Uuid;

    fn test_uuid() -> Uuid {
        Uuid::parse_str("550e8400-e29b-41d4-a716-446655440001").unwrap()
    }
    use crate::director::Director;
    use crate::storage::MemoryImageStore;
    use std::net::Ipv4Addr;
    use std::sync::Arc;
    use tempfile::tempdir;
    use tokio::sync::Mutex;

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
        });

        (state, temp_dir)
    }

    #[tokio::test]
    async fn test_enrich_interfaces_with_dhcp_info_no_leases() {
        let (state, _temp_dir) = create_test_state().await;

        let interfaces = vec![NetworkInterface {
            interface_name: "eth0".to_string(),
            mac_address: "aa:bb:cc:dd:ee:ff".to_string(),
            ip_address: None,
            is_primary: false,
            network_id: None,
            disabled: false,
            warning_label: None,
        }];

        let enriched = enrich_interfaces_with_dhcp_info(&state, interfaces).await;

        assert_eq!(enriched.len(), 1);
        assert_eq!(enriched[0].interface_name, "eth0");
        assert_eq!(enriched[0].ip_address, None);
        assert_eq!(enriched[0].network_id, None);
    }

    #[tokio::test]
    async fn test_enrich_interfaces_with_dhcp_info_with_lease() {
        let (state, _temp_dir) = create_test_state().await;

        // Create a DHCP lease
        let mac = "aa:bb:cc:dd:ee:ff";
        let ip: Ipv4Addr = "10.0.0.100".parse().unwrap();
        state
            .dhcp_store
            .create_or_update_lease_with_network(
                mac,
                &ip,
                None,
                crate::dhcp::LeaseState::Active,
                3600,
                1,
            )
            .await
            .unwrap();

        let interfaces = vec![NetworkInterface {
            interface_name: "eth0".to_string(),
            mac_address: mac.to_string(),
            ip_address: None,
            is_primary: false,
            network_id: None,
            disabled: false,
            warning_label: None,
        }];

        let enriched = enrich_interfaces_with_dhcp_info(&state, interfaces).await;

        assert_eq!(enriched.len(), 1);
        assert_eq!(enriched[0].interface_name, "eth0");
        assert_eq!(enriched[0].ip_address, Some("10.0.0.100".to_string()));
        assert_eq!(enriched[0].network_id, Some(1));
    }

    #[tokio::test]
    async fn test_detect_and_mark_duplicates_no_duplicates() {
        let (state, _temp_dir) = create_test_state().await;

        let uuid = test_uuid();
        state
            .director
            .register_device(&uuid, crate::operating_systems::Architecture::X86_64)
            .await
            .unwrap();

        let mut interfaces = vec![NetworkInterface {
            interface_name: "eth0".to_string(),
            mac_address: "aa:bb:cc:dd:ee:ff".to_string(),
            ip_address: Some("10.0.0.100".to_string()),
            is_primary: false,
            network_id: Some(1),
            disabled: false,
            warning_label: None,
        }];

        detect_and_mark_duplicates(&state, &uuid, &mut interfaces).await;

        assert_eq!(interfaces[0].disabled, false);
        assert_eq!(interfaces[0].warning_label, None);
    }

    #[tokio::test]
    async fn test_complete_pending_devices_for_interfaces_no_pending() {
        let (state, _temp_dir) = create_test_state().await;

        let uuid = test_uuid();
        let interfaces = vec![NetworkInterface {
            interface_name: "eth0".to_string(),
            mac_address: "aa:bb:cc:dd:ee:ff".to_string(),
            ip_address: Some("10.0.0.100".to_string()),
            is_primary: false,
            network_id: Some(1),
            disabled: false,
            warning_label: None,
        }];

        // Should not panic or error
        complete_pending_devices_for_interfaces(&state, &uuid, &interfaces).await;
    }

    #[tokio::test]
    async fn test_complete_pending_devices_for_interfaces_with_pending() {
        let (state, _temp_dir) = create_test_state().await;

        let mac = "aa:bb:cc:dd:ee:ff";
        let uuid = test_uuid();

        // Register the device first (required due to foreign key constraint)
        state
            .director
            .register_device(&uuid, crate::operating_systems::Architecture::X86_64)
            .await
            .unwrap();

        // Create pending device
        state.director.create_pending_device(mac, 1).await.unwrap();

        // Verify it exists
        let pending = state
            .director
            .find_pending_device_by_mac(mac)
            .await
            .unwrap();
        assert!(pending.is_some());

        let interfaces = vec![NetworkInterface {
            interface_name: "eth0".to_string(),
            mac_address: mac.to_string(),
            ip_address: Some("10.0.0.100".to_string()),
            is_primary: false,
            network_id: Some(1),
            disabled: false,
            warning_label: None,
        }];

        complete_pending_devices_for_interfaces(&state, &uuid, &interfaces).await;

        // Verify pending device was completed (removed)
        let pending = state
            .director
            .find_pending_device_by_mac(mac)
            .await
            .unwrap();
        assert!(pending.is_none());
    }
}
