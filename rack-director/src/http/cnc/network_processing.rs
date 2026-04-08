use crate::database::Connection;
use crate::director::NetworkInterface;
use crate::{dhcp, director};
use log::warn;
use uuid::Uuid;

/// Enriches network interfaces with IP addresses and network IDs from DHCP leases.
///
/// Takes a list of network interfaces (typically from device hardware discovery) and
/// backfills IP addresses and network IDs by looking up DHCP leases for each MAC address.
/// This allows us to associate discovered hardware interfaces with their network assignments.
///
/// # Arguments
/// * `conn` - An open database connection
/// * `interfaces` - Vector of NetworkInterface objects to enrich
///
/// # Returns
/// A new vector of NetworkInterface objects with IP addresses and network IDs populated
/// from DHCP lease data where available.
pub async fn enrich_interfaces_with_dhcp_info(
    conn: &Connection,
    interfaces: Vec<NetworkInterface>,
) -> Vec<NetworkInterface> {
    let mut enriched = Vec::new();

    for mut nic in interfaces {
        // Look up DHCP lease for this MAC
        if let Ok(Some(lease)) = dhcp::store::get_lease_by_mac(conn, &nic.mac_address).await {
            log::info!(
                "Backfilling IP {} and network_id {} for NIC {} (MAC {})",
                lease.ip_address,
                lease.network_id.unwrap_or(-1),
                nic.interface_name,
                nic.mac_address
            );
            nic.ip_address = Some(lease.ip_address.clone());
            nic.network_id = lease.network_id;

            // Create static reservation for this interface
            if let Some(network_id) = lease.network_id
                && let Err(e) = dhcp::store::create_or_update_static_reservation(
                    conn,
                    network_id,
                    &nic.mac_address,
                    &lease.ip_address,
                    None,
                )
                .await
            {
                log::warn!(
                    "Couldn't create static reservation for MAC {}: {}",
                    nic.mac_address,
                    e
                );
            }
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
/// * `conn` - An open database connection
/// * `device_uuid` - UUID of the device being checked
/// * `interfaces` - Mutable reference to interfaces to check and potentially mark as disabled
///
/// # Side Effects
/// Modifies interfaces in-place, setting `disabled = true` and `warning_label = Some(...)`
/// for any interface with a duplicate MAC on the same network.
pub async fn detect_and_mark_duplicates(
    conn: &Connection,
    device_uuid: &Uuid,
    interfaces: &mut [NetworkInterface],
) {
    for nic in interfaces.iter_mut() {
        // Only check for duplicates if the interface has a network_id
        if let Some(network_id) = nic.network_id {
            match director::store::find_duplicate_macs_on_network(
                conn,
                &nic.mac_address,
                network_id,
                device_uuid,
            )
            .await
            {
                Ok(duplicates) if !duplicates.is_empty() => {
                    // Get network name for warning message
                    let network_name = match dhcp::store::get_network(conn, network_id).await {
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
/// * `conn` - An open database connection
/// * `device_uuid` - UUID of the device that owns these interfaces
/// * `interfaces` - Network interfaces to check against pending devices
///
/// # Side Effects
/// Completes pending device registration for any matching MAC addresses, linking them
/// to the provided device UUID.
pub async fn complete_pending_devices_for_interfaces(
    conn: &Connection,
    device_uuid: &Uuid,
    interfaces: &[NetworkInterface],
) {
    for nic in interfaces {
        if let Err(e) =
            director::store::complete_pending_device(conn, &nic.mac_address, device_uuid).await
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
    use crate::database::{DatabaseConnectionFactory, run_migrations};
    use crate::director::Director;
    use crate::test_database_path;
    use std::net::Ipv4Addr;
    use uuid::Uuid;

    fn test_uuid() -> Uuid {
        Uuid::parse_str("550e8400-e29b-41d4-a716-446655440001").unwrap()
    }

    async fn create_test_conn(path: String) -> Connection {
        let factory = DatabaseConnectionFactory::new(std::path::PathBuf::from(path));
        run_migrations(&factory).await.unwrap()
    }

    /// Helper to create a test network for tests that need DHCP functionality
    async fn create_test_network(conn: &Connection) -> i64 {
        let network = dhcp::store::create_network(
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

        dhcp::store::create_pool(conn, network.id, "Test Pool", "10.0.0.100", "10.0.0.200")
            .await
            .unwrap();

        network.id
    }

    #[tokio::test]
    async fn test_enrich_interfaces_with_dhcp_info_no_leases() {
        let conn = create_test_conn(test_database_path!()).await;

        let interfaces = vec![NetworkInterface {
            interface_name: "eth0".to_string(),
            mac_address: "aa:bb:cc:dd:ee:ff".to_string(),
            ip_address: None,
            network_id: None,
            speed_mbps: None,
            disabled: false,
            warning_label: None,
        }];

        let enriched = enrich_interfaces_with_dhcp_info(&conn, interfaces).await;

        assert_eq!(enriched.len(), 1);
        assert_eq!(enriched[0].interface_name, "eth0");
        assert_eq!(enriched[0].ip_address, None);
        assert_eq!(enriched[0].network_id, None);
    }

    #[tokio::test]
    async fn test_enrich_interfaces_with_dhcp_info_with_lease() {
        let conn = create_test_conn(test_database_path!()).await;
        let network_id = create_test_network(&conn).await;

        // Create a DHCP lease
        let mac = "aa:bb:cc:dd:ee:ff";
        let ip: Ipv4Addr = "10.0.0.100".parse().unwrap();
        dhcp::store::create_or_update_lease_with_network(
            &conn,
            mac,
            &ip,
            None,
            crate::dhcp::LeaseState::Active,
            3600,
            network_id,
        )
        .await
        .unwrap();

        let interfaces = vec![NetworkInterface {
            interface_name: "eth0".to_string(),
            mac_address: mac.to_string(),
            ip_address: None,
            network_id: None,
            speed_mbps: None,
            disabled: false,
            warning_label: None,
        }];

        let enriched = enrich_interfaces_with_dhcp_info(&conn, interfaces).await;

        assert_eq!(enriched.len(), 1);
        assert_eq!(enriched[0].interface_name, "eth0");
        assert_eq!(enriched[0].ip_address, Some("10.0.0.100".to_string()));
        assert_eq!(enriched[0].network_id, Some(network_id));
    }

    #[tokio::test]
    async fn test_detect_and_mark_duplicates_no_duplicates() {
        let conn = create_test_conn(test_database_path!()).await;

        let uuid = test_uuid();
        Director::new(&conn)
            .register_device(&uuid, crate::director::Architecture::X86_64)
            .await
            .unwrap();

        let mut interfaces = vec![NetworkInterface {
            interface_name: "eth0".to_string(),
            mac_address: "aa:bb:cc:dd:ee:ff".to_string(),
            ip_address: Some("10.0.0.100".to_string()),
            network_id: Some(1),
            speed_mbps: Some(10000),
            disabled: false,
            warning_label: None,
        }];

        detect_and_mark_duplicates(&conn, &uuid, &mut interfaces).await;

        assert!(!interfaces[0].disabled);
        assert_eq!(interfaces[0].warning_label, None);
    }

    #[tokio::test]
    async fn test_complete_pending_devices_for_interfaces_no_pending() {
        let conn = create_test_conn(test_database_path!()).await;

        let uuid = test_uuid();
        let interfaces = vec![NetworkInterface {
            interface_name: "eth0".to_string(),
            mac_address: "aa:bb:cc:dd:ee:ff".to_string(),
            ip_address: Some("10.0.0.100".to_string()),
            network_id: Some(1),
            speed_mbps: Some(10000),
            disabled: false,
            warning_label: None,
        }];

        // Should not panic or error
        complete_pending_devices_for_interfaces(&conn, &uuid, &interfaces).await;
    }

    #[tokio::test]
    async fn test_complete_pending_devices_for_interfaces_with_pending() {
        let conn = create_test_conn(test_database_path!()).await;
        let network_id = create_test_network(&conn).await;

        let mac = "aa:bb:cc:dd:ee:ff";
        let uuid = test_uuid();

        let director = Director::new(&conn);

        // Register the device first (required due to foreign key constraint)
        director
            .register_device(&uuid, crate::director::Architecture::X86_64)
            .await
            .unwrap();

        // Create pending device
        director
            .create_pending_device(mac, network_id)
            .await
            .unwrap();

        // Verify it exists
        let pending = director.find_pending_device_by_mac(mac).await.unwrap();
        assert!(pending.is_some());

        let interfaces = vec![NetworkInterface {
            interface_name: "eth0".to_string(),
            mac_address: mac.to_string(),
            ip_address: Some("10.0.0.100".to_string()),
            network_id: Some(network_id),
            speed_mbps: Some(10000),
            disabled: false,
            warning_label: None,
        }];

        complete_pending_devices_for_interfaces(&conn, &uuid, &interfaces).await;

        // Verify pending device was completed (removed)
        let pending = director::store::find_pending_device_by_mac(&conn, mac)
            .await
            .unwrap();
        assert!(pending.is_none());
    }

    #[tokio::test]
    async fn test_enrich_interfaces_creates_static_reservations() {
        let conn = create_test_conn(test_database_path!()).await;
        let network_id = create_test_network(&conn).await;

        // Create DHCP leases for multiple MACs
        let mac1 = "aa:bb:cc:dd:ee:01";
        let mac2 = "aa:bb:cc:dd:ee:02";
        let ip1: Ipv4Addr = "10.0.0.101".parse().unwrap();
        let ip2: Ipv4Addr = "10.0.0.102".parse().unwrap();

        dhcp::store::create_or_update_lease_with_network(
            &conn,
            mac1,
            &ip1,
            None,
            crate::dhcp::LeaseState::Active,
            3600,
            network_id,
        )
        .await
        .unwrap();

        dhcp::store::create_or_update_lease_with_network(
            &conn,
            mac2,
            &ip2,
            None,
            crate::dhcp::LeaseState::Active,
            3600,
            network_id,
        )
        .await
        .unwrap();

        // Create interfaces without IP info
        let interfaces = vec![
            NetworkInterface {
                interface_name: "eth0".to_string(),
                mac_address: mac1.to_string(),
                ip_address: None,
                network_id: None,
                speed_mbps: None,
                disabled: false,
                warning_label: None,
            },
            NetworkInterface {
                interface_name: "eth1".to_string(),
                mac_address: mac2.to_string(),
                ip_address: None,
                network_id: None,
                speed_mbps: None,
                disabled: false,
                warning_label: None,
            },
        ];

        // Enrich interfaces - should create static reservations
        let enriched = enrich_interfaces_with_dhcp_info(&conn, interfaces).await;

        // Verify interfaces were enriched
        assert_eq!(enriched.len(), 2);
        assert_eq!(enriched[0].ip_address, Some("10.0.0.101".to_string()));
        assert_eq!(enriched[1].ip_address, Some("10.0.0.102".to_string()));

        // Verify static reservations were created
        let res1 = dhcp::store::get_static_reservation(&conn, network_id, mac1)
            .await
            .unwrap();
        assert!(res1.is_some());
        assert_eq!(res1.unwrap().ip_address, "10.0.0.101");

        let res2 = dhcp::store::get_static_reservation(&conn, network_id, mac2)
            .await
            .unwrap();
        assert!(res2.is_some());
        assert_eq!(res2.unwrap().ip_address, "10.0.0.102");
    }
}
