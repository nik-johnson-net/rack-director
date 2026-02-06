use super::super::{AppState, error::Error as HttpError};
use super::validation::{validate_create_network_request, validate_update_network_request};
use crate::dhcp::{DhcpNetwork, DhcpPool, Lease, StaticReservation};
use crate::director::Device;
use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{delete, get, post, put},
};
use common::Ipv4Subnet;
use serde::Deserialize;
use std::sync::Arc;
use uuid::Uuid;

pub fn routes(state: Arc<AppState>) -> Router {
    Router::new()
        // Networks
        .route("/ui/dhcp/networks", get(list_networks).post(create_network))
        .route(
            "/ui/dhcp/networks/{id}",
            get(get_network).put(update_network).delete(delete_network),
        )
        // Pools
        .route(
            "/ui/dhcp/networks/{network_id}/pools",
            get(list_pools).post(create_pool),
        )
        .route("/ui/dhcp/pools/{id}", put(update_pool).delete(delete_pool))
        // Static Reservations
        .route(
            "/ui/dhcp/networks/{network_id}/static-reservations",
            get(list_static_reservations).post(create_static_reservation),
        )
        .route(
            "/ui/dhcp/static-reservations/{id}",
            delete(delete_static_reservation),
        )
        // Leases
        .route(
            "/ui/dhcp/networks/{network_id}/leases",
            get(list_leases_by_network),
        )
        .route("/ui/dhcp/leases/{id}/make-static", post(make_lease_static))
        .with_state(state)
}

// ========== Request/Response Types ==========

#[derive(Debug, Deserialize)]
pub struct CreateNetworkRequest {
    pub name: String,
    pub subnet: String,
    pub gateway: String,
    pub dns_servers: Vec<String>,
    pub lease_duration: u32,
    pub relay_agent_address: Option<String>,
    #[serde(default)]
    pub enable_autodiscovery: bool,
}

#[derive(Debug, Deserialize)]
pub struct UpdateNetworkRequest {
    pub name: Option<String>,
    pub subnet: Option<String>,
    pub gateway: Option<String>,
    pub dns_servers: Option<Vec<String>>,
    pub lease_duration: Option<u32>,
    pub relay_agent_address: Option<String>,
    pub enable_autodiscovery: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct CreatePoolRequest {
    pub name: String,
    pub range_start: String,
    pub range_end: String,
}

#[derive(Debug, Deserialize)]
pub struct UpdatePoolRequest {
    pub name: Option<String>,
    pub range_start: Option<String>,
    pub range_end: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreateStaticReservationRequest {
    pub mac_address: String,
    pub ip_address: String,
    pub hostname: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct MakeStaticRequest {
    pub ip_address: Option<String>,
    pub hostname: Option<String>,
}

// ========== Network Handlers ==========

/// List all DHCP networks
async fn list_networks(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<DhcpNetwork>>, HttpError> {
    let networks = state.dhcp_store.list_networks().await?;
    Ok(Json(networks))
}

/// Get a specific network by ID
async fn get_network(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<DhcpNetwork>, HttpError> {
    let network = state.dhcp_store.get_network(id).await?;
    Ok(Json(network))
}

/// Create a new DHCP network
async fn create_network(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateNetworkRequest>,
) -> Result<(StatusCode, Json<DhcpNetwork>), HttpError> {
    log::debug!("create network request: {:?}", req);
    // Validate request
    if let Err(errors) = validate_create_network_request(&req, &state.dhcp_store).await {
        return Err(HttpError::ValidationError(errors));
    }

    let network = state
        .dhcp_store
        .create_network(
            &req.name,
            &req.subnet,
            &req.gateway,
            &req.dns_servers,
            req.lease_duration,
            req.relay_agent_address.as_deref(),
            req.enable_autodiscovery,
        )
        .await?;

    Ok((StatusCode::CREATED, Json(network)))
}

/// Update an existing network
async fn update_network(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(req): Json<UpdateNetworkRequest>,
) -> Result<Json<DhcpNetwork>, HttpError> {
    log::debug!("update network request: {:?}", req);
    // Validate request
    if let Err(errors) = validate_update_network_request(id, &req, &state.dhcp_store).await {
        return Err(HttpError::ValidationError(errors));
    }

    let network = state
        .dhcp_store
        .update_network(
            id,
            req.name.as_deref(),
            req.subnet.as_deref(),
            req.gateway.as_deref(),
            req.dns_servers.as_deref(),
            req.lease_duration,
            req.relay_agent_address
                .as_deref()
                .map(|opt| if opt.is_empty() { None } else { Some(opt) }),
            req.enable_autodiscovery,
        )
        .await?;

    Ok(Json(network))
}

/// Delete a network
async fn delete_network(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<StatusCode, HttpError> {
    state.dhcp_store.delete_network(id).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ========== Pool Handlers ==========

/// List all pools for a network
async fn list_pools(
    State(state): State<Arc<AppState>>,
    Path(network_id): Path<i64>,
) -> Result<Json<Vec<DhcpPool>>, HttpError> {
    let pools = state.dhcp_store.list_pools_for_network(network_id).await?;
    Ok(Json(pools))
}

/// Create a new pool in a network
async fn create_pool(
    State(state): State<Arc<AppState>>,
    Path(network_id): Path<i64>,
    Json(req): Json<CreatePoolRequest>,
) -> Result<(StatusCode, Json<DhcpPool>), HttpError> {
    let pool = state
        .dhcp_store
        .create_pool(network_id, &req.name, &req.range_start, &req.range_end)
        .await?;

    Ok((StatusCode::CREATED, Json(pool)))
}

/// Update an existing pool
async fn update_pool(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(req): Json<UpdatePoolRequest>,
) -> Result<Json<DhcpPool>, HttpError> {
    let pool = state
        .dhcp_store
        .update_pool(
            id,
            req.name.as_deref(),
            req.range_start.as_deref(),
            req.range_end.as_deref(),
        )
        .await?;

    Ok(Json(pool))
}

/// Delete a pool
async fn delete_pool(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<StatusCode, HttpError> {
    state.dhcp_store.delete_pool(id).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ========== Static Reservation Handlers ==========

/// List all static reservations for a network
async fn list_static_reservations(
    State(state): State<Arc<AppState>>,
    Path(network_id): Path<i64>,
) -> Result<Json<Vec<StaticReservation>>, HttpError> {
    let reservations = state
        .dhcp_store
        .list_static_reservations(network_id)
        .await?;
    Ok(Json(reservations))
}

/// Create a new static reservation
async fn create_static_reservation(
    State(state): State<Arc<AppState>>,
    Path(network_id): Path<i64>,
    Json(req): Json<CreateStaticReservationRequest>,
) -> Result<(StatusCode, Json<StaticReservation>), HttpError> {
    // Fetch the network to get the subnet
    let network = state.dhcp_store.get_network(network_id).await?;

    // Validate the IP is within the subnet
    let subnet: Ipv4Subnet = network.subnet.parse()?;
    if !subnet.ip_in_range(req.ip_address.parse()?) {
        return Err(HttpError::BadRequest(
            "ip address not within network".to_string(),
        ));
    }

    let reservation = state
        .dhcp_store
        .create_static_reservation(
            network_id,
            &req.mac_address,
            &req.ip_address,
            req.hostname.as_deref(),
        )
        .await?;

    Ok((StatusCode::CREATED, Json(reservation)))
}

/// Delete a static reservation
async fn delete_static_reservation(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<StatusCode, HttpError> {
    state.dhcp_store.delete_static_reservation(id).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ========== Lease Handlers ==========

/// List all leases for a specific network
///
/// This endpoint associates leases with devices by searching for the lease's MAC address
/// across all device network interfaces (including secondary NICs and BMCs).
async fn list_leases_by_network(
    State(state): State<Arc<AppState>>,
    Path(network_id): Path<i64>,
) -> Result<Json<Vec<Lease>>, HttpError> {
    // Fetch leases for the network
    let mut leases = state.dhcp_store.get_leases_by_network(network_id).await?;

    // Fetch all devices for MAC lookup
    let devices = state.director.get_all_devices().await?;

    // For each lease without a device_uuid, try to find it by MAC address
    for lease in &mut leases {
        if lease.device_uuid.is_none() {
            // Search all device NICs (including secondary NICs and BMCs) for this MAC
            if let Some(device_uuid) = find_device_uuid_by_mac(&devices, &lease.mac_address) {
                lease.device_uuid = Some(device_uuid);
            }
        }
    }

    Ok(Json(leases))
}

/// Convert a dynamic lease to a static reservation
async fn make_lease_static(
    State(state): State<Arc<AppState>>,
    Path(lease_id): Path<i64>,
    Json(req): Json<MakeStaticRequest>,
) -> Result<(StatusCode, Json<StaticReservation>), HttpError> {
    // Get the lease by ID
    let lease = state
        .dhcp_store
        .get_lease_by_id(lease_id)
        .await?
        .ok_or_else(|| HttpError::NotFound(format!("Lease {} not found", lease_id)))?;

    // Verify the lease has a network_id
    let network_id = lease
        .network_id
        .ok_or_else(|| HttpError::BadRequest("Lease has no associated network".to_string()))?;

    // Fetch the network to get the subnet
    let network = state.dhcp_store.get_network(network_id).await?;

    // Determine the IP address to use (from request or lease)
    let ip_address = req.ip_address.as_deref().unwrap_or(&lease.ip_address);

    // Validate the IP is within the subnet
    let subnet: Ipv4Subnet = network.subnet.parse()?;
    if !subnet.ip_in_range(ip_address.parse()?) {
        return Err(HttpError::BadRequest(
            "ip address not within network".to_string(),
        ));
    }

    // Use hostname from request if provided, otherwise use lease hostname
    let hostname = req.hostname.or(lease.hostname);

    // Create static reservation
    let reservation = state
        .dhcp_store
        .create_static_reservation(
            network_id,
            &lease.mac_address,
            ip_address,
            hostname.as_deref(),
        )
        .await?;

    Ok((StatusCode::CREATED, Json(reservation)))
}

// ========== Helper Functions ==========

/// Find a device UUID by searching for a MAC address across all device network interfaces.
///
/// This function searches for the given MAC address in:
/// 1. Device network_interfaces array (all NICs)
/// 2. Legacy mac_address field
/// 3. BMC mac_address field
///
/// The MAC comparison is case-insensitive.
///
/// # Arguments
/// * `devices` - Slice of all devices to search through
/// * `mac` - The MAC address to search for (in any case format)
///
/// # Returns
/// * `Some(Uuid)` - The UUID of the device with this MAC
/// * `None` - If no device has this MAC address
fn find_device_uuid_by_mac(devices: &[Device], mac: &str) -> Option<Uuid> {
    let mac_lower = mac.to_lowercase();

    for device in devices {
        // Check network_interfaces array (primary and secondary NICs)
        for interface in &device.attributes.network_interfaces {
            if interface.mac_address.to_lowercase() == mac_lower {
                return Some(device.uuid);
            }
        }

        // Check legacy mac_address field
        if let Some(legacy_mac) = &device.attributes.mac_address
            && legacy_mac.to_lowercase() == mac_lower
        {
            return Some(device.uuid);
        }

        // Check BMC MAC address
        if let Some(bmc) = &device.attributes.bmc
            && bmc.mac_address.to_lowercase() == mac_lower
        {
            return Some(device.uuid);
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::operating_systems::Architecture;
    use common::device_attributes::{BmcInfo, DeviceAttributes, NetworkInterface};

    fn create_test_device(
        uuid: Uuid,
        network_interfaces: Vec<NetworkInterface>,
        mac_address: Option<String>,
        bmc: Option<BmcInfo>,
    ) -> Device {
        Device {
            uuid,
            architecture: Architecture::X86_64,
            lifecycle: None,
            role_id: None,
            attributes: DeviceAttributes {
                network_interfaces,
                mac_address,
                bmc,
                ..Default::default()
            },
            created_at: None,
            first_seen_at: None,
            last_seen_at: None,
        }
    }

    #[test]
    fn test_find_device_by_primary_nic_mac() {
        let uuid = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440001").unwrap();
        let devices = vec![create_test_device(
            uuid,
            vec![NetworkInterface {
                interface_name: "eth0".to_string(),
                mac_address: "aa:bb:cc:dd:ee:01".to_string(),
                ip_address: Some("10.0.0.100".to_string()),
                is_primary: true,
                network_id: Some(1),
                disabled: false,
                warning_label: None,
            }],
            None,
            None,
        )];

        let result = find_device_uuid_by_mac(&devices, "aa:bb:cc:dd:ee:01");
        assert_eq!(result, Some(uuid));
    }

    #[test]
    fn test_find_device_by_secondary_nic_mac() {
        let uuid = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440002").unwrap();
        let devices = vec![create_test_device(
            uuid,
            vec![
                NetworkInterface {
                    interface_name: "eth0".to_string(),
                    mac_address: "aa:bb:cc:dd:ee:01".to_string(),
                    ip_address: Some("10.0.0.100".to_string()),
                    is_primary: true,
                    network_id: Some(1),
                    disabled: false,
                    warning_label: None,
                },
                NetworkInterface {
                    interface_name: "eth1".to_string(),
                    mac_address: "aa:bb:cc:dd:ee:02".to_string(),
                    ip_address: Some("10.0.0.101".to_string()),
                    is_primary: false,
                    network_id: Some(1),
                    disabled: false,
                    warning_label: None,
                },
            ],
            None,
            None,
        )];

        // Should find device by secondary NIC MAC
        let result = find_device_uuid_by_mac(&devices, "aa:bb:cc:dd:ee:02");
        assert_eq!(result, Some(uuid));
    }

    #[test]
    fn test_find_device_by_legacy_mac() {
        let uuid = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440003").unwrap();
        let devices = vec![create_test_device(
            uuid,
            vec![],
            Some("aa:bb:cc:dd:ee:ff".to_string()),
            None,
        )];

        let result = find_device_uuid_by_mac(&devices, "aa:bb:cc:dd:ee:ff");
        assert_eq!(result, Some(uuid));
    }

    #[test]
    fn test_find_device_by_bmc_mac() {
        let uuid = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440004").unwrap();
        let devices = vec![create_test_device(
            uuid,
            vec![],
            None,
            Some(BmcInfo {
                mac_address: "11:22:33:44:55:66".to_string(),
                ip_address: Some("10.0.1.10".to_string()),
                ip_address_source: Some("DHCP".to_string()),
            }),
        )];

        let result = find_device_uuid_by_mac(&devices, "11:22:33:44:55:66");
        assert_eq!(result, Some(uuid));
    }

    #[test]
    fn test_find_device_by_mac_case_insensitive() {
        let uuid = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440005").unwrap();
        let devices = vec![create_test_device(
            uuid,
            vec![NetworkInterface {
                interface_name: "eth0".to_string(),
                mac_address: "AA:BB:CC:DD:EE:01".to_string(), // Uppercase in storage
                ip_address: Some("10.0.0.100".to_string()),
                is_primary: true,
                network_id: Some(1),
                disabled: false,
                warning_label: None,
            }],
            None,
            None,
        )];

        // Should find with lowercase query
        let result = find_device_uuid_by_mac(&devices, "aa:bb:cc:dd:ee:01");
        assert_eq!(result, Some(uuid));

        // Should find with uppercase query
        let result = find_device_uuid_by_mac(&devices, "AA:BB:CC:DD:EE:01");
        assert_eq!(result, Some(uuid));

        // Should find with mixed case query
        let result = find_device_uuid_by_mac(&devices, "Aa:Bb:Cc:Dd:Ee:01");
        assert_eq!(result, Some(uuid));
    }

    #[test]
    fn test_find_device_unknown_mac_returns_none() {
        let uuid = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440006").unwrap();
        let devices = vec![create_test_device(
            uuid,
            vec![NetworkInterface {
                interface_name: "eth0".to_string(),
                mac_address: "aa:bb:cc:dd:ee:01".to_string(),
                ip_address: Some("10.0.0.100".to_string()),
                is_primary: true,
                network_id: Some(1),
                disabled: false,
                warning_label: None,
            }],
            None,
            None,
        )];

        let result = find_device_uuid_by_mac(&devices, "ff:ff:ff:ff:ff:ff");
        assert_eq!(result, None);
    }

    #[test]
    fn test_find_device_with_multiple_devices() {
        let uuid1 = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440001").unwrap();
        let uuid2 = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440002").unwrap();
        let uuid3 = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440003").unwrap();

        let devices = vec![
            create_test_device(
                uuid1,
                vec![NetworkInterface {
                    interface_name: "eth0".to_string(),
                    mac_address: "aa:bb:cc:dd:ee:01".to_string(),
                    ip_address: Some("10.0.0.100".to_string()),
                    is_primary: true,
                    network_id: Some(1),
                    disabled: false,
                    warning_label: None,
                }],
                None,
                None,
            ),
            create_test_device(
                uuid2,
                vec![NetworkInterface {
                    interface_name: "eth0".to_string(),
                    mac_address: "aa:bb:cc:dd:ee:02".to_string(),
                    ip_address: Some("10.0.0.101".to_string()),
                    is_primary: true,
                    network_id: Some(1),
                    disabled: false,
                    warning_label: None,
                }],
                None,
                None,
            ),
            create_test_device(
                uuid3,
                vec![],
                None,
                Some(BmcInfo {
                    mac_address: "11:22:33:44:55:66".to_string(),
                    ip_address: Some("10.0.1.10".to_string()),
                    ip_address_source: Some("DHCP".to_string()),
                }),
            ),
        ];

        // Find first device
        let result = find_device_uuid_by_mac(&devices, "aa:bb:cc:dd:ee:01");
        assert_eq!(result, Some(uuid1));

        // Find second device
        let result = find_device_uuid_by_mac(&devices, "aa:bb:cc:dd:ee:02");
        assert_eq!(result, Some(uuid2));

        // Find third device by BMC
        let result = find_device_uuid_by_mac(&devices, "11:22:33:44:55:66");
        assert_eq!(result, Some(uuid3));
    }

    #[test]
    fn test_find_device_empty_list() {
        let devices = vec![];
        let result = find_device_uuid_by_mac(&devices, "aa:bb:cc:dd:ee:01");
        assert_eq!(result, None);
    }
}
