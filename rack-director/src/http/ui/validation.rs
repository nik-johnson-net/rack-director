use common::Ipv4Subnet;
use std::collections::HashMap;
use std::net::Ipv4Addr;
use std::str::FromStr;

// ============================================================================
// GENERIC VALIDATORS - Reusable across all endpoints
// ============================================================================

/// Validate that a string is non-empty
pub fn validate_required(value: &str, field_name: &str) -> Option<String> {
    if value.is_empty() {
        Some(format!("{} is required", field_name))
    } else {
        None
    }
}

/// Validate string length constraints
pub fn validate_string_length(value: &str, max_len: usize, field_name: &str) -> Option<String> {
    if value.len() > max_len {
        Some(format!(
            "{} must be less than {} characters",
            field_name, max_len
        ))
    } else {
        None
    }
}

/// Validate that a string is a valid IPv4 address
pub fn validate_ipv4_address(ip: &str) -> Option<String> {
    match Ipv4Addr::from_str(ip) {
        Ok(_) => None,
        Err(_) => Some("Must be a valid IPv4 address".to_string()),
    }
}

/// Validate that a string is a valid CIDR subnet
pub fn validate_cidr_subnet(subnet: &str) -> Result<Ipv4Subnet, String> {
    Ipv4Subnet::from_str(subnet).map_err(|e| format!("Invalid CIDR notation: {}", e))
}

/// Validate that an IP address is within a subnet
pub fn validate_ip_in_subnet(ip: &str, subnet: &Ipv4Subnet) -> Option<String> {
    match Ipv4Addr::from_str(ip) {
        Ok(ip_addr) => {
            if subnet.ip_in_range(ip_addr) {
                None
            } else {
                Some("IP address must be within the subnet range".to_string())
            }
        }
        Err(_) => Some("Must be a valid IPv4 address".to_string()),
    }
}

/// Validate a list of IPv4 addresses
pub fn validate_ipv4_list(ips: &[String], field_name: &str, min_count: usize) -> Option<String> {
    if ips.len() < min_count {
        return Some(format!("At least {} {} required", min_count, field_name));
    }

    for ip in ips {
        if Ipv4Addr::from_str(ip).is_err() {
            return Some(format!("'{}' is not a valid IPv4 address", ip));
        }
    }

    None
}

/// Validate numeric range
pub fn validate_u32_range(value: u32, min: u32, max: u32, field_name: &str) -> Option<String> {
    if value < min {
        Some(format!("{} must be at least {}", field_name, min))
    } else if value > max {
        Some(format!("{} must be less than {}", field_name, max))
    } else {
        None
    }
}

/// Validate hostname according to RFC 1123
/// - Required field (non-empty)
/// - Max length 253 characters
/// - Valid characters: alphanumeric, hyphens, dots
/// - No leading or trailing hyphens
pub fn validate_hostname(hostname: &str) -> Result<(), HashMap<String, String>> {
    let mut errors = ValidationErrors::new();

    // Hostname must not be empty
    errors.add_if_err("hostname", validate_required(hostname, "Hostname"));

    // Hostname max length (RFC 1123: 253 chars total)
    errors.add_if_err(
        "hostname",
        validate_string_length(hostname, 253, "Hostname"),
    );

    // Hostname format: alphanumeric and hyphens/dots, no leading/trailing hyphens
    if !hostname.is_empty() {
        let valid = hostname
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.');

        if !valid {
            errors.add_error(
                "hostname",
                "Hostname must contain only letters, numbers, hyphens, and dots".to_string(),
            );
        } else {
            // Check for leading/trailing hyphens
            if hostname.starts_with('-') || hostname.ends_with('-') {
                errors.add_error(
                    "hostname",
                    "Hostname cannot start or end with a hyphen".to_string(),
                );
            }
        }
    }

    errors.into_result()
}

// ============================================================================
// HELPER TYPE - For building validation errors
// ============================================================================

/// Helper for accumulating validation errors
pub struct ValidationErrors {
    errors: HashMap<String, String>,
}

impl ValidationErrors {
    pub fn new() -> Self {
        Self {
            errors: HashMap::new(),
        }
    }

    /// Add an error for a field if the validation result contains an error
    pub fn add_if_err(&mut self, field: &str, result: Option<String>) {
        if let Some(err) = result {
            self.errors.insert(field.to_string(), err);
        }
    }

    /// Add an error for a field directly
    pub fn add_error(&mut self, field: &str, message: String) {
        self.errors.insert(field.to_string(), message);
    }

    /// Check if there are any errors and return Result
    pub fn into_result(self) -> Result<(), HashMap<String, String>> {
        if self.errors.is_empty() {
            Ok(())
        } else {
            Err(self.errors)
        }
    }
}

// ============================================================================
// NETWORK-SPECIFIC VALIDATORS
// ============================================================================

use super::networks::{CreateNetworkRequest, UpdateNetworkRequest};
use crate::dhcp::DhcpStore;

/// Validate create network request using the generic validators
pub async fn validate_create_network_request(
    req: &CreateNetworkRequest,
    dhcp_store: &DhcpStore,
) -> Result<(), HashMap<String, String>> {
    let mut errors = ValidationErrors::new();

    // Validate name using generic validators
    errors.add_if_err("name", validate_required(&req.name, "Network name"));
    errors.add_if_err(
        "name",
        validate_string_length(&req.name, 255, "Network name"),
    );

    // Check for duplicate network name
    match dhcp_store.get_network_by_name(&req.name).await {
        Ok(Some(_)) => {
            errors.add_error(
                "name",
                "A network with this name already exists".to_string(),
            );
        }
        Ok(None) => {}
        Err(e) => {
            log::warn!("Failed to check for duplicate network name: {}", e);
        }
    }

    // Validate subnet and gateway together
    match validate_cidr_subnet(&req.subnet) {
        Err(err) => {
            errors.add_if_err("subnet", Some(err));
        }
        Ok(subnet) => {
            // Only validate gateway in subnet if subnet is valid
            errors.add_if_err("gateway", validate_ipv4_address(&req.gateway));
            errors.add_if_err("gateway", validate_ip_in_subnet(&req.gateway, &subnet));
        }
    }

    // Validate DNS servers using generic list validator
    errors.add_if_err(
        "dns_servers",
        validate_ipv4_list(&req.dns_servers, "DNS server", 1),
    );

    // Validate lease duration using generic range validator
    const ONE_YEAR_SECONDS: u32 = 31536000;
    errors.add_if_err(
        "lease_duration",
        validate_u32_range(req.lease_duration, 1, ONE_YEAR_SECONDS, "Lease duration"),
    );

    // Validate relay agent if present
    if let Some(ref relay) = req.relay_agent_address {
        // Validate format if not empty
        if !relay.is_empty() {
            errors.add_if_err("relay_agent_address", validate_ipv4_address(relay));
        }
    }

    // Check for duplicate relay agent address
    let relay_for_check = req
        .relay_agent_address
        .as_ref()
        .and_then(|r| if r.is_empty() { None } else { Some(r.as_str()) });

    match dhcp_store
        .get_network_by_relay_string(relay_for_check)
        .await
    {
        Ok(Some(_)) => {
            if relay_for_check.is_none() {
                errors.add_error(
                    "relay_agent_address",
                    "A Default L2 network (no relay agent) already exists".to_string(),
                );
            } else {
                errors.add_error(
                    "relay_agent_address",
                    "A network with this relay agent address already exists".to_string(),
                );
            }
        }
        Ok(None) => {}
        Err(e) => {
            log::warn!("Failed to check for duplicate relay agent address: {}", e);
        }
    }

    errors.into_result()
}

/// Validate update network request using the generic validators
/// Excludes the network being updated from duplicate checks
pub async fn validate_update_network_request(
    network_id: i64,
    req: &UpdateNetworkRequest,
    dhcp_store: &DhcpStore,
) -> Result<(), HashMap<String, String>> {
    let mut errors = ValidationErrors::new();

    // Validate name if provided
    if let Some(ref name) = req.name {
        errors.add_if_err("name", validate_required(name, "Network name"));
        errors.add_if_err("name", validate_string_length(name, 255, "Network name"));

        // Check for duplicate network name (excluding current network)
        match dhcp_store.get_network_by_name(name).await {
            Ok(Some(existing_network)) => {
                if existing_network.id != network_id {
                    errors.add_error(
                        "name",
                        "A network with this name already exists".to_string(),
                    );
                }
            }
            Ok(None) => {}
            Err(e) => {
                log::warn!("Failed to check for duplicate network name: {}", e);
            }
        }
    }

    // Validate subnet and gateway together if either is provided
    if req.subnet.is_some() || req.gateway.is_some() {
        // Get the current network to fill in missing values
        let current_network = match dhcp_store.get_network(network_id).await {
            Ok(net) => net,
            Err(e) => {
                log::warn!("Failed to fetch current network for validation: {}", e);
                return errors.into_result();
            }
        };

        let subnet_str = req.subnet.as_deref().unwrap_or(&current_network.subnet);
        let gateway_str = req.gateway.as_deref().unwrap_or(&current_network.gateway);

        match validate_cidr_subnet(subnet_str) {
            Err(err) => {
                errors.add_if_err("subnet", Some(err));
            }
            Ok(subnet) => {
                errors.add_if_err("gateway", validate_ipv4_address(gateway_str));
                errors.add_if_err("gateway", validate_ip_in_subnet(gateway_str, &subnet));
            }
        }
    }

    // Validate DNS servers if provided
    if let Some(ref dns_servers) = req.dns_servers {
        errors.add_if_err(
            "dns_servers",
            validate_ipv4_list(dns_servers, "DNS server", 1),
        );
    }

    // Validate lease duration if provided
    if let Some(lease_duration) = req.lease_duration {
        const ONE_YEAR_SECONDS: u32 = 31536000;
        errors.add_if_err(
            "lease_duration",
            validate_u32_range(lease_duration, 1, ONE_YEAR_SECONDS, "Lease duration"),
        );
    }

    // Validate relay agent if provided
    if let Some(relay) = &req.relay_agent_address {
        // Validate format if not empty
        if !relay.is_empty() {
            errors.add_if_err("relay_agent_address", validate_ipv4_address(relay));
        }

        // Check for duplicate relay agent address (excluding current network)
        let relay_for_check = if relay.is_empty() {
            None
        } else {
            Some(relay.as_str())
        };

        match dhcp_store
            .get_network_by_relay_string(relay_for_check)
            .await
        {
            Ok(Some(existing_network)) => {
                if existing_network.id != network_id {
                    if relay_for_check.is_none() {
                        errors.add_error(
                            "relay_agent_address",
                            "A Default L2 network (no relay agent) already exists".to_string(),
                        );
                    } else {
                        errors.add_error(
                            "relay_agent_address",
                            "A network with this relay agent address already exists".to_string(),
                        );
                    }
                }
            }
            Ok(None) => {}
            Err(e) => {
                log::warn!("Failed to check for duplicate relay agent address: {}", e);
            }
        }
    }

    errors.into_result()
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dhcp::DhcpStore;
    use std::sync::Arc;
    use tempfile::tempdir;
    use tokio::sync::Mutex;

    async fn create_test_store() -> (DhcpStore, tempfile::TempDir) {
        let temp_dir = tempdir().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let conn = crate::database::open(db_path).unwrap();
        (DhcpStore::new(Arc::new(Mutex::new(conn))), temp_dir)
    }

    // Test generic validators
    #[test]
    fn test_validate_required() {
        assert!(validate_required("value", "Field").is_none());
        assert!(validate_required("", "Field").is_some());
    }

    #[test]
    fn test_validate_string_length() {
        assert!(validate_string_length("short", 10, "Field").is_none());
        let long_string = "x".repeat(100);
        assert!(validate_string_length(&long_string, 10, "Field").is_some());
    }

    #[test]
    fn test_validate_ipv4_address() {
        assert!(validate_ipv4_address("192.168.1.1").is_none());
        assert!(validate_ipv4_address("invalid").is_some());
    }

    #[test]
    fn test_validate_cidr_subnet() {
        assert!(validate_cidr_subnet("192.168.1.0/24").is_ok());
        assert!(validate_cidr_subnet("invalid").is_err());
        assert!(validate_cidr_subnet("192.168.1.0/33").is_err());
    }

    #[test]
    fn test_validate_ipv4_list() {
        assert!(validate_ipv4_list(&["8.8.8.8".to_string()], "DNS", 1).is_none());
        assert!(validate_ipv4_list(&[], "DNS", 1).is_some());
        assert!(validate_ipv4_list(&["invalid".to_string()], "DNS", 1).is_some());
    }

    #[test]
    fn test_validate_u32_range() {
        assert!(validate_u32_range(50, 1, 100, "Value").is_none());
        assert!(validate_u32_range(0, 1, 100, "Value").is_some());
        assert!(validate_u32_range(101, 1, 100, "Value").is_some());
    }

    #[test]
    fn test_validation_errors_builder() {
        let mut errors = ValidationErrors::new();
        errors.add_if_err("field1", Some("Error 1".to_string()));
        errors.add_if_err("field2", None);
        errors.add_if_err("field3", Some("Error 3".to_string()));

        let result = errors.into_result();
        assert!(result.is_err());
        let err_map = result.unwrap_err();
        assert_eq!(err_map.len(), 2);
        assert!(err_map.contains_key("field1"));
        assert!(err_map.contains_key("field3"));
    }

    #[tokio::test]
    async fn test_validate_create_network_request_valid() {
        let (store, _temp_dir) = create_test_store().await;
        let req = CreateNetworkRequest {
            name: "Test Network".to_string(),
            subnet: "192.168.1.0/24".to_string(),
            gateway: "192.168.1.1".to_string(),
            dns_servers: vec!["8.8.8.8".to_string()],
            lease_duration: 86400,
            relay_agent_address: Some("10.0.0.2".to_string()),
            enable_autodiscovery: false,
        };

        assert!(validate_create_network_request(&req, &store).await.is_ok());
    }

    #[tokio::test]
    async fn test_validate_create_network_request_empty_name() {
        let (store, _temp_dir) = create_test_store().await;
        let req = CreateNetworkRequest {
            name: "".to_string(),
            subnet: "192.168.1.0/24".to_string(),
            gateway: "192.168.1.1".to_string(),
            dns_servers: vec!["8.8.8.8".to_string()],
            lease_duration: 86400,
            relay_agent_address: Some("10.0.0.2".to_string()),
            enable_autodiscovery: false,
        };

        let result = validate_create_network_request(&req, &store).await;
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors.contains_key("name"));
    }

    #[tokio::test]
    async fn test_validate_create_network_request_duplicate_name() {
        let (store, _temp_dir) = create_test_store().await;

        // Create a network with a specific name
        store
            .create_network(
                "Existing Network",
                "192.168.1.0/24",
                "192.168.1.1",
                &["8.8.8.8".to_string()],
                86400,
                Some("10.0.0.2"),
                false,
            )
            .await
            .unwrap();

        // Try to create another network with the same name
        let req = CreateNetworkRequest {
            name: "Existing Network".to_string(),
            subnet: "192.168.2.0/24".to_string(),
            gateway: "192.168.2.1".to_string(),
            dns_servers: vec!["8.8.8.8".to_string()],
            lease_duration: 86400,
            relay_agent_address: Some("10.0.0.3".to_string()),
            enable_autodiscovery: false,
        };

        let result = validate_create_network_request(&req, &store).await;
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors.contains_key("name"));
        assert!(errors.get("name").unwrap().contains("already exists"));
    }

    #[tokio::test]
    async fn test_validate_create_network_request_duplicate_relay_agent() {
        let (store, _temp_dir) = create_test_store().await;

        // Create a network with a specific relay agent
        store
            .create_network(
                "Network 1",
                "192.168.1.0/24",
                "192.168.1.1",
                &["8.8.8.8".to_string()],
                86400,
                Some("10.0.0.2"),
                false,
            )
            .await
            .unwrap();

        // Try to create another network with the same relay agent
        let req = CreateNetworkRequest {
            name: "Network 2".to_string(),
            subnet: "192.168.2.0/24".to_string(),
            gateway: "192.168.2.1".to_string(),
            dns_servers: vec!["8.8.8.8".to_string()],
            lease_duration: 86400,
            relay_agent_address: Some("10.0.0.2".to_string()),
            enable_autodiscovery: false,
        };

        let result = validate_create_network_request(&req, &store).await;
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors.contains_key("relay_agent_address"));
        assert!(
            errors
                .get("relay_agent_address")
                .unwrap()
                .contains("already exists")
        );
    }

    #[tokio::test]
    async fn test_validate_create_network_request_duplicate_default_l2() {
        let (store, _temp_dir) = create_test_store().await;

        // Create a default L2 network (no relay agent) first
        store
            .create_network(
                "Default L2",
                "10.0.0.0/24",
                "10.0.0.1",
                &["8.8.8.8".to_string()],
                86400,
                None,
                false,
            )
            .await
            .unwrap();

        // Try to create another Default L2 network (no relay agent)
        let req = CreateNetworkRequest {
            name: "Another Default L2".to_string(),
            subnet: "192.168.2.0/24".to_string(),
            gateway: "192.168.2.1".to_string(),
            dns_servers: vec!["8.8.8.8".to_string()],
            lease_duration: 86400,
            relay_agent_address: None,
            enable_autodiscovery: false,
        };

        let result = validate_create_network_request(&req, &store).await;
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors.contains_key("relay_agent_address"));
        assert!(
            errors
                .get("relay_agent_address")
                .unwrap()
                .contains("Default L2")
        );
    }

    #[tokio::test]
    async fn test_validate_create_network_request_invalid_subnet() {
        let (store, _temp_dir) = create_test_store().await;
        let req = CreateNetworkRequest {
            name: "Test".to_string(),
            subnet: "invalid".to_string(),
            gateway: "192.168.1.1".to_string(),
            dns_servers: vec!["8.8.8.8".to_string()],
            lease_duration: 86400,
            relay_agent_address: Some("10.0.0.2".to_string()),
            enable_autodiscovery: false,
        };

        let result = validate_create_network_request(&req, &store).await;
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors.contains_key("subnet"));
    }

    #[tokio::test]
    async fn test_validate_create_network_request_gateway_out_of_subnet() {
        let (store, _temp_dir) = create_test_store().await;
        let req = CreateNetworkRequest {
            name: "Test".to_string(),
            subnet: "192.168.1.0/24".to_string(),
            gateway: "10.0.0.1".to_string(),
            dns_servers: vec!["8.8.8.8".to_string()],
            lease_duration: 86400,
            relay_agent_address: Some("10.0.0.2".to_string()),
            enable_autodiscovery: false,
        };

        let result = validate_create_network_request(&req, &store).await;
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors.contains_key("gateway"));
    }

    #[tokio::test]
    async fn test_validate_create_network_request_no_dns() {
        let (store, _temp_dir) = create_test_store().await;
        let req = CreateNetworkRequest {
            name: "Test".to_string(),
            subnet: "192.168.1.0/24".to_string(),
            gateway: "192.168.1.1".to_string(),
            dns_servers: vec![],
            lease_duration: 86400,
            relay_agent_address: Some("10.0.0.2".to_string()),
            enable_autodiscovery: false,
        };

        let result = validate_create_network_request(&req, &store).await;
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors.contains_key("dns_servers"));
    }

    #[tokio::test]
    async fn test_validate_create_network_request_invalid_lease_duration() {
        let (store, _temp_dir) = create_test_store().await;
        let req = CreateNetworkRequest {
            name: "Test".to_string(),
            subnet: "192.168.1.0/24".to_string(),
            gateway: "192.168.1.1".to_string(),
            dns_servers: vec!["8.8.8.8".to_string()],
            lease_duration: 0,
            relay_agent_address: Some("10.0.0.2".to_string()),
            enable_autodiscovery: false,
        };

        let result = validate_create_network_request(&req, &store).await;
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors.contains_key("lease_duration"));
    }

    #[tokio::test]
    async fn test_validate_create_network_request_invalid_relay() {
        let (store, _temp_dir) = create_test_store().await;
        let req = CreateNetworkRequest {
            name: "Test".to_string(),
            subnet: "192.168.1.0/24".to_string(),
            gateway: "192.168.1.1".to_string(),
            dns_servers: vec!["8.8.8.8".to_string()],
            lease_duration: 86400,
            relay_agent_address: Some("invalid".to_string()),
            enable_autodiscovery: false,
        };

        let result = validate_create_network_request(&req, &store).await;
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors.contains_key("relay_agent_address"));
    }

    #[tokio::test]
    async fn test_validate_create_network_request_multiple_errors() {
        let (store, _temp_dir) = create_test_store().await;
        let req = CreateNetworkRequest {
            name: "".to_string(),
            subnet: "invalid".to_string(),
            gateway: "10.0.0.1".to_string(),
            dns_servers: vec![],
            lease_duration: 0,
            relay_agent_address: None,
            enable_autodiscovery: false,
        };

        let result = validate_create_network_request(&req, &store).await;
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors.contains_key("name"));
        assert!(errors.contains_key("subnet"));
        assert!(errors.contains_key("dns_servers"));
        assert!(errors.contains_key("lease_duration"));
    }

    // ========== Update Network Validation Tests ==========

    #[tokio::test]
    async fn test_validate_update_network_request_valid() {
        let (store, _temp_dir) = create_test_store().await;

        // Create a network to update
        let network = store
            .create_network(
                "Original Network",
                "192.168.1.0/24",
                "192.168.1.1",
                &["8.8.8.8".to_string()],
                86400,
                Some("10.0.0.2"),
                false,
            )
            .await
            .unwrap();

        let req = UpdateNetworkRequest {
            name: Some("Updated Network".to_string()),
            subnet: None,
            gateway: None,
            dns_servers: None,
            lease_duration: Some(7200),
            relay_agent_address: None,
            enable_autodiscovery: None,
        };

        assert!(
            validate_update_network_request(network.id, &req, &store)
                .await
                .is_ok()
        );
    }

    #[tokio::test]
    async fn test_validate_update_network_request_duplicate_name_different_network() {
        let (store, _temp_dir) = create_test_store().await;

        // Create two networks
        let network1 = store
            .create_network(
                "Network 1",
                "192.168.1.0/24",
                "192.168.1.1",
                &["8.8.8.8".to_string()],
                86400,
                Some("10.0.0.2"),
                false,
            )
            .await
            .unwrap();

        store
            .create_network(
                "Network 2",
                "192.168.2.0/24",
                "192.168.2.1",
                &["8.8.8.8".to_string()],
                86400,
                Some("10.0.0.3"),
                false,
            )
            .await
            .unwrap();

        // Try to update network1 to have the same name as network2
        let req = UpdateNetworkRequest {
            name: Some("Network 2".to_string()),
            subnet: None,
            gateway: None,
            dns_servers: None,
            lease_duration: None,
            relay_agent_address: None,
            enable_autodiscovery: None,
        };

        let result = validate_update_network_request(network1.id, &req, &store).await;
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors.contains_key("name"));
        assert_eq!(
            errors.get("name").unwrap(),
            "A network with this name already exists"
        );
    }

    #[tokio::test]
    async fn test_validate_update_network_request_same_name_same_network() {
        let (store, _temp_dir) = create_test_store().await;

        // Create a network
        let network = store
            .create_network(
                "Test Network",
                "192.168.1.0/24",
                "192.168.1.1",
                &["8.8.8.8".to_string()],
                86400,
                Some("10.0.0.2"),
                false,
            )
            .await
            .unwrap();

        // Update with the same name (should be allowed)
        let req = UpdateNetworkRequest {
            name: Some("Test Network".to_string()),
            subnet: None,
            gateway: None,
            dns_servers: None,
            lease_duration: None,
            relay_agent_address: None,
            enable_autodiscovery: None,
        };

        assert!(
            validate_update_network_request(network.id, &req, &store)
                .await
                .is_ok()
        );
    }

    #[tokio::test]
    async fn test_validate_update_network_request_duplicate_relay_agent() {
        let (store, _temp_dir) = create_test_store().await;

        // Create two networks
        let network1 = store
            .create_network(
                "Network 1",
                "192.168.1.0/24",
                "192.168.1.1",
                &["8.8.8.8".to_string()],
                86400,
                Some("10.0.0.2"),
                false,
            )
            .await
            .unwrap();

        store
            .create_network(
                "Network 2",
                "192.168.2.0/24",
                "192.168.2.1",
                &["8.8.8.8".to_string()],
                86400,
                Some("10.0.0.3"),
                false,
            )
            .await
            .unwrap();

        // Try to update network1 to have the same relay agent as network2
        let req = UpdateNetworkRequest {
            name: None,
            subnet: None,
            gateway: None,
            dns_servers: None,
            lease_duration: None,
            relay_agent_address: Some("10.0.0.3".to_string()),
            enable_autodiscovery: None,
        };

        let result = validate_update_network_request(network1.id, &req, &store).await;
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors.contains_key("relay_agent_address"));
    }

    #[tokio::test]
    async fn test_validate_update_network_request_same_relay_agent_same_network() {
        let (store, _temp_dir) = create_test_store().await;

        // Create a network
        let network = store
            .create_network(
                "Test Network",
                "192.168.1.0/24",
                "192.168.1.1",
                &["8.8.8.8".to_string()],
                86400,
                Some("10.0.0.2"),
                false,
            )
            .await
            .unwrap();

        // Update with the same relay agent (should be allowed)
        let req = UpdateNetworkRequest {
            name: None,
            subnet: None,
            gateway: None,
            dns_servers: None,
            lease_duration: None,
            relay_agent_address: Some("10.0.0.2".to_string()),
            enable_autodiscovery: None,
        };

        assert!(
            validate_update_network_request(network.id, &req, &store)
                .await
                .is_ok()
        );
    }

    #[tokio::test]
    async fn test_validate_update_network_request_gateway_out_of_new_subnet() {
        let (store, _temp_dir) = create_test_store().await;

        // Create a network
        let network = store
            .create_network(
                "Test Network",
                "192.168.1.0/24",
                "192.168.1.1",
                &["8.8.8.8".to_string()],
                86400,
                Some("10.0.0.2"),
                false,
            )
            .await
            .unwrap();

        // Update subnet to a range that doesn't contain the gateway
        let req = UpdateNetworkRequest {
            name: None,
            subnet: Some("10.0.0.0/24".to_string()),
            gateway: None, // Will use existing gateway 192.168.1.1
            dns_servers: None,
            lease_duration: None,
            relay_agent_address: None,
            enable_autodiscovery: None,
        };

        let result = validate_update_network_request(network.id, &req, &store).await;
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors.contains_key("gateway"));
    }

    #[tokio::test]
    async fn test_validate_update_network_request_invalid_dns() {
        let (store, _temp_dir) = create_test_store().await;

        // Create a network
        let network = store
            .create_network(
                "Test Network",
                "192.168.1.0/24",
                "192.168.1.1",
                &["8.8.8.8".to_string()],
                86400,
                Some("10.0.0.2"),
                false,
            )
            .await
            .unwrap();

        // Update with invalid DNS
        let req = UpdateNetworkRequest {
            name: None,
            subnet: None,
            gateway: None,
            dns_servers: Some(vec!["invalid".to_string()]),
            lease_duration: None,
            relay_agent_address: None,
            enable_autodiscovery: None,
        };

        let result = validate_update_network_request(network.id, &req, &store).await;
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors.contains_key("dns_servers"));
    }

    // ========== Hostname Validation Tests ==========

    #[test]
    fn test_validate_hostname_valid() {
        assert!(validate_hostname("server-01").is_ok());
        assert!(validate_hostname("web.server").is_ok());
        assert!(validate_hostname("app-server-01.example.com").is_ok());
        assert!(validate_hostname("s").is_ok());
        assert!(validate_hostname("server123").is_ok());
        assert!(validate_hostname("my-server.local").is_ok());
    }

    #[test]
    fn test_validate_hostname_empty() {
        let result = validate_hostname("");
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors.contains_key("hostname"));
        assert!(errors.get("hostname").unwrap().contains("required"));
    }

    #[test]
    fn test_validate_hostname_too_long() {
        // Create a hostname that's 254 characters (exceeds 253 limit)
        let long_hostname = "a".repeat(254);
        let result = validate_hostname(&long_hostname);
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors.contains_key("hostname"));
        assert!(
            errors
                .get("hostname")
                .unwrap()
                .contains("less than 253 characters")
        );
    }

    #[test]
    fn test_validate_hostname_max_length_valid() {
        // Exactly 253 characters should be valid
        let max_hostname = "a".repeat(253);
        assert!(validate_hostname(&max_hostname).is_ok());
    }

    #[test]
    fn test_validate_hostname_invalid_characters() {
        let result = validate_hostname("server_01");
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors.contains_key("hostname"));
        assert!(
            errors
                .get("hostname")
                .unwrap()
                .contains("only letters, numbers, hyphens, and dots")
        );

        // Test other invalid characters
        assert!(validate_hostname("server@example").is_err());
        assert!(validate_hostname("server#01").is_err());
        assert!(validate_hostname("server 01").is_err());
        assert!(validate_hostname("server!01").is_err());
    }

    #[test]
    fn test_validate_hostname_leading_hyphen() {
        let result = validate_hostname("-server");
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors.contains_key("hostname"));
        assert!(
            errors
                .get("hostname")
                .unwrap()
                .contains("cannot start or end with a hyphen")
        );
    }

    #[test]
    fn test_validate_hostname_trailing_hyphen() {
        let result = validate_hostname("server-");
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors.contains_key("hostname"));
        assert!(
            errors
                .get("hostname")
                .unwrap()
                .contains("cannot start or end with a hyphen")
        );
    }

    #[test]
    fn test_validate_hostname_both_hyphens() {
        let result = validate_hostname("-server-");
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors.contains_key("hostname"));
    }

    #[test]
    fn test_validate_hostname_hyphen_in_middle() {
        // Hyphens in the middle are allowed
        assert!(validate_hostname("my-server").is_ok());
        assert!(validate_hostname("app-server-01").is_ok());
    }

    #[test]
    fn test_validate_hostname_dots_allowed() {
        assert!(validate_hostname("server.example.com").is_ok());
        assert!(validate_hostname("web.local").is_ok());
    }

    #[test]
    fn test_validate_hostname_numbers_allowed() {
        assert!(validate_hostname("server01").is_ok());
        assert!(validate_hostname("123server").is_ok());
        assert!(validate_hostname("123").is_ok());
    }

    #[test]
    fn test_validate_hostname_multiple_errors() {
        // Empty string triggers required error only
        let result = validate_hostname("");
        assert!(result.is_err());
        let errors = result.unwrap_err();
        // Should have at least the "required" error
        assert!(errors.contains_key("hostname"));
    }
}
