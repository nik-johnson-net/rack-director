use crate::{database::Connection, dhcp, director::Director};
use log::warn;
use std::net::SocketAddr;
use uuid::Uuid;

/// Resolves the MAC address for a device from query parameter or DHCP lookup.
///
/// Attempts to determine the MAC address in two ways:
/// 1. Directly from the MAC query parameter (if provided and non-empty)
/// 2. By looking up the client IP address in DHCP leases (fallback for older clients)
///
/// # Arguments
/// * `conn` - Database connection
/// * `mac_param` - Optional MAC address from query parameter
/// * `client_addr` - Client socket address (IP + port)
///
/// # Returns
/// * `Some(String)` - MAC address if found via parameter or DHCP lookup
/// * `None` - If MAC cannot be determined
pub async fn resolve_mac_address(
    conn: &Connection,
    mac_param: Option<&String>,
    client_addr: SocketAddr,
) -> Option<String> {
    match mac_param {
        Some(mac) if !mac.is_empty() => Some(mac.clone()),
        _ => {
            // Fallback: Look up MAC address from client IP (may not work in all network setups)
            let client_ip = client_addr.ip().to_string();
            if let Ok(leases) = dhcp::store::get_all_leases(conn).await {
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
/// * `conn` - Database connection
/// * `device_uuid` - UUID of the device to register
/// * `mac_address` - Optional MAC address to link with pending devices
pub async fn register_and_start_discovery(
    conn: &Connection,
    device_uuid: &Uuid,
    mac_address: Option<&String>,
) {
    let director = Director::new(conn);

    // Check for pending device
    if let Some(mac) = mac_address
        && let Ok(Some(_)) = director.find_pending_device_by_mac(mac).await
    {
        log::info!(
            "Completing pending device for MAC {} with UUID {}",
            mac,
            device_uuid
        );
    }

    // Register device
    if let Err(e) = director
        .register_device(device_uuid, crate::director::Architecture::X86_64)
        .await
    {
        warn!("Couldn't register device {}: {}", device_uuid, e);
        return;
    }

    // Complete pending device link
    if let Some(mac) = mac_address
        && let Err(e) = director.complete_pending_device(mac, device_uuid).await
    {
        warn!("Couldn't complete pending device: {}", e);
    }

    // Create static DHCP reservation from active lease
    if let Some(mac) = mac_address
        && let Ok(Some(lease)) = dhcp::store::get_lease_by_mac(conn, mac).await
        && let Some(network_id) = lease.network_id
    {
        let hostname = director
            .get_device(device_uuid)
            .await
            .ok()
            .and_then(|d| d.attributes.hostname);

        if let Err(e) = dhcp::store::create_or_update_static_reservation(
            conn,
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
    if let Err(e) = director
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
    use crate::test_database_path;
    use std::net::Ipv4Addr;
    use uuid::Uuid;

    fn test_uuid() -> Uuid {
        Uuid::parse_str("550e8400-e29b-41d4-a716-446655440001").unwrap()
    }

    async fn create_test_conn(path: String) -> Connection {
        let factory =
            crate::database::DatabaseConnectionFactory::new(std::path::PathBuf::from(path));
        crate::database::run_migrations(&factory).await.unwrap()
    }

    /// Helper to create a test network for tests that need DHCP functionality.
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
    async fn test_resolve_mac_address_from_parameter() {
        let conn = create_test_conn(test_database_path!()).await;
        let mac = "aa:bb:cc:dd:ee:ff".to_string();
        let addr = "127.0.0.1:1234".parse().unwrap();

        let result = resolve_mac_address(&conn, Some(&mac), addr).await;
        assert_eq!(result, Some(mac));
    }

    #[tokio::test]
    async fn test_resolve_mac_address_empty_parameter() {
        let conn = create_test_conn(test_database_path!()).await;
        let empty_mac = "".to_string();
        let addr = "127.0.0.1:1234".parse().unwrap();

        let result = resolve_mac_address(&conn, Some(&empty_mac), addr).await;
        // Should attempt DHCP lookup, which will return None with no leases
        assert_eq!(result, None);
    }

    #[tokio::test]
    async fn test_resolve_mac_address_from_dhcp() {
        let conn = create_test_conn(test_database_path!()).await;
        let network_id = create_test_network(&conn).await;
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

        let addr: SocketAddr = format!("{}:1234", ip).parse().unwrap();
        let result = resolve_mac_address(&conn, None, addr).await;
        assert_eq!(result, Some(mac.to_string()));
    }

    #[tokio::test]
    async fn test_register_and_start_discovery() {
        let conn = create_test_conn(test_database_path!()).await;
        let uuid = test_uuid();

        // Verify device doesn't exist
        assert!(!Director::new(&conn).device_exists(&uuid).await.unwrap());

        register_and_start_discovery(&conn, &uuid, None).await;

        // Verify device was registered
        assert!(Director::new(&conn).device_exists(&uuid).await.unwrap());

        // Verify lifecycle was started (device should be in New state)
        let lifecycle = Director::new(&conn)
            .get_device_lifecycle(&uuid)
            .await
            .unwrap();
        assert_eq!(lifecycle, Some(crate::lifecycle::DeviceLifecycle::New));

        // Verify discovery plan was created
        let plan = Director::new(&conn)
            .get_active_plan_for_device(&uuid)
            .await
            .unwrap();
        assert!(plan.is_some());
    }

    #[tokio::test]
    async fn test_register_and_start_discovery_with_pending_device() {
        let conn = create_test_conn(test_database_path!()).await;
        let network_id = create_test_network(&conn).await;
        let uuid = test_uuid();
        let mac = "aa:bb:cc:dd:ee:ff".to_string();

        // Create pending device
        Director::new(&conn)
            .create_pending_device(&mac, network_id)
            .await
            .unwrap();

        // Verify pending device exists
        let pending = Director::new(&conn)
            .find_pending_device_by_mac(&mac)
            .await
            .unwrap();
        assert!(pending.is_some());

        register_and_start_discovery(&conn, &uuid, Some(&mac)).await;

        // Verify device was registered
        assert!(Director::new(&conn).device_exists(&uuid).await.unwrap());

        // Verify pending device was completed (removed)
        let pending = Director::new(&conn)
            .find_pending_device_by_mac(&mac)
            .await
            .unwrap();
        assert!(pending.is_none());
    }

    #[tokio::test]
    async fn test_register_and_start_discovery_creates_static_reservation() {
        let conn = create_test_conn(test_database_path!()).await;
        let network_id = create_test_network(&conn).await;
        let uuid = test_uuid();
        let mac = "aa:bb:cc:dd:ee:11".to_string();

        // Create a DHCP lease for this MAC (without device_uuid initially)
        let ip: Ipv4Addr = "10.0.0.150".parse().unwrap();
        dhcp::store::create_or_update_lease_with_network(
            &conn,
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
        register_and_start_discovery(&conn, &uuid, Some(&mac)).await;

        // Verify static reservation was created
        let reservation = dhcp::store::get_static_reservation(&conn, network_id, &mac)
            .await
            .unwrap();
        assert!(reservation.is_some());
        let r = reservation.unwrap();
        assert_eq!(r.mac_address, mac);
        assert_eq!(r.ip_address, "10.0.0.150");
        assert_eq!(r.network_id, network_id);

        // Verify hostname is included
        let device = Director::new(&conn).get_device(&uuid).await.unwrap();
        assert_eq!(r.hostname, device.attributes.hostname);
    }
}
