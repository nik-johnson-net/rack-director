use crate::patch::Patch;
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

/// Reject an explicitly-supplied blank value for a nullable field.
///
/// `None` (absent) is accepted. `null` is the only way to clear a nullable column; an empty (or
/// whitespace-only) string is a client error rather than a clear request. For a [`Patch`]
/// field pass `patch.value().map(String::as_str)` so only `Set` values are checked.
pub fn validate_not_blank(value: Option<&str>, field_name: &str) -> Option<String> {
    value
        .filter(|v| v.trim().is_empty())
        .map(|_| format!("{} cannot be empty; send null to clear.", field_name))
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

/// Validate MAC address format (`XX:XX:XX:XX:XX:XX` where X is a hex digit).
///
/// Returns `Some(error_message)` when the format is invalid, `None` when valid.
pub fn validate_mac_address(mac: &str) -> Option<String> {
    if common::is_valid_mac(mac) {
        None
    } else {
        Some("MAC address must be in the format XX:XX:XX:XX:XX:XX".to_string())
    }
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

/// Validate a device hostname as a **single** DNS label (RFC 1123).
///
/// Dots are rejected: the domain belongs to the device's network, and install-script rendering
/// appends it unconditionally (`{hostname}.{domain}`), so a dotted hostname would install with a
/// doubled domain. Enforcing bareness here keeps that concatenation correct by construction.
///
/// - Required field (non-empty)
/// - Max length 63 characters (RFC 1123 label limit)
/// - Valid characters: ASCII alphanumerics and hyphens
/// - No leading or trailing hyphens
pub fn validate_hostname(hostname: &str) -> Result<(), HashMap<String, String>> {
    let mut errors = ValidationErrors::new();

    // Hostname must not be empty
    errors.add_if_err("hostname", validate_required(hostname, "Hostname"));

    // A single label is capped at 63 characters (RFC 1123), not the 253-char FQDN limit
    errors.add_if_err("hostname", validate_string_length(hostname, 63, "Hostname"));

    if !hostname.is_empty() {
        if hostname.contains('.') {
            errors.add_error(
                "hostname",
                "Hostname must be a single label; the domain comes from the network".to_string(),
            );
        } else {
            errors.add_if_err(
                "hostname",
                validate_dns_label(hostname, "Hostname", "Hostname"),
            );
        }
    }

    errors.into_result()
}

/// Validate a DNS domain name (RFC 1123): dot-separated labels of ASCII alphanumerics and
/// hyphens, each 1-63 chars, no leading/trailing hyphen, no empty labels, <=253 chars total.
///
/// An empty string is the caller's concern — treat it as "no domain" before calling this.
pub fn validate_domain_name(domain: &str) -> Option<String> {
    if let Some(err) = validate_string_length(domain, 253, "Domain name") {
        return Some(err);
    }

    for label in domain.split('.') {
        if label.is_empty() {
            return Some("Domain name cannot contain an empty label".to_string());
        }
        if label.len() > 63 {
            return Some(format!(
                "Domain name label '{}' must be 63 characters or fewer",
                label
            ));
        }
        let label_subject = format!("Domain name label '{}'", label);
        if let Some(err) = validate_dns_label(label, "Domain name labels", &label_subject) {
            return Some(err);
        }
    }

    None
}

/// RFC 1123 label content rules shared by hostnames and domain labels: ASCII alphanumerics and
/// hyphens only, no leading/trailing hyphen. Length and emptiness are the caller's concern.
///
/// `chars_subject` and `edge_subject` are the nouns used in the respective error messages.
fn validate_dns_label(label: &str, chars_subject: &str, edge_subject: &str) -> Option<String> {
    if !label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        Some(format!(
            "{} must contain only letters, numbers, and hyphens",
            chars_subject
        ))
    } else if label.starts_with('-') || label.ends_with('-') {
        Some(format!(
            "{} cannot start or end with a hyphen",
            edge_subject
        ))
    } else {
        None
    }
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

use crate::{database::Connection, dhcp};

use super::networks::{CreateNetworkRequest, UpdateNetworkRequest};

/// Record a blank-value error for `field` when `value` is present but blank.
///
/// Returns `true` when the value is acceptable (absent, or present and non-blank), so callers
/// can skip format checks that would otherwise pile a second error onto the same field.
fn accept_non_blank(
    errors: &mut ValidationErrors,
    field: &str,
    value: Option<&str>,
    field_name: &str,
) -> bool {
    let error = validate_not_blank(value, field_name);
    let accepted = error.is_none();
    errors.add_if_err(field, error);
    accepted
}

/// Validate a network name and check it is not already taken by a *different* network.
async fn validate_network_name(
    conn: &Connection,
    name: &str,
    exclude_network_id: Option<i64>,
    errors: &mut ValidationErrors,
) {
    errors.add_if_err("name", validate_required(name, "Network name"));
    errors.add_if_err("name", validate_string_length(name, 255, "Network name"));

    match dhcp::store::get_network_by_name(conn, name).await {
        Ok(Some(existing)) if Some(existing.id) != exclude_network_id => {
            errors.add_error(
                "name",
                "A network with this name already exists".to_string(),
            );
        }
        Ok(_) => {}
        Err(e) => {
            log::warn!("Failed to check for duplicate network name: {}", e);
        }
    }
}

/// Add an error when `relay` is already claimed by a network other than `exclude_network_id`.
///
/// A `None` relay means the network is local L2; only one such network may exist.
async fn check_relay_uniqueness(
    conn: &Connection,
    relay: Option<&str>,
    exclude_network_id: Option<i64>,
    errors: &mut ValidationErrors,
) {
    let existing = match dhcp::store::get_network_by_relay_string(conn, relay).await {
        Ok(Some(network)) => network,
        Ok(None) => return,
        Err(e) => {
            log::warn!("Failed to check for duplicate relay agent address: {}", e);
            return;
        }
    };

    if Some(existing.id) == exclude_network_id {
        return;
    }

    let message = if relay.is_none() {
        "A network without a relay agent already exists. Only one local L2 network is allowed."
    } else {
        "A network with this relay agent address already exists"
    };
    errors.add_error("relay_agent_address", message.to_string());
}

/// Validate a lease duration in seconds.
fn validate_lease_duration(lease_duration: u32) -> Option<String> {
    const ONE_YEAR_SECONDS: u32 = 31536000;
    validate_u32_range(lease_duration, 1, ONE_YEAR_SECONDS, "Lease duration")
}

/// Validate the gateway (when there is one) against the parsed subnet, recording errors for
/// both `subnet` and `gateway` as appropriate.
fn validate_subnet_and_gateway(
    subnet_str: &str,
    gateway: Option<&str>,
    errors: &mut ValidationErrors,
) {
    match validate_cidr_subnet(subnet_str) {
        Err(err) => errors.add_if_err("subnet", Some(err)),
        Ok(subnet) => {
            if let Some(gw) = gateway {
                errors.add_if_err("gateway", validate_ipv4_address(gw));
                errors.add_if_err("gateway", validate_ip_in_subnet(gw, &subnet));
            }
        }
    }
}

/// Validate an optional domain name: not blank, and a well-formed RFC 1123 domain.
///
/// No duplicate check -- multiple networks may legitimately share one domain.
fn validate_domain(errors: &mut ValidationErrors, domain: Option<&str>) {
    if accept_non_blank(errors, "domain_name", domain, "Domain name")
        && let Some(domain) = domain
    {
        errors.add_if_err("domain_name", validate_domain_name(domain));
    }
}

/// Validate an optional relay agent address: not blank, a valid IPv4 address, and not already
/// claimed by a network other than `exclude_network_id`. `None` means local L2.
async fn validate_relay(
    conn: &Connection,
    relay: Option<&str>,
    exclude_network_id: Option<i64>,
    errors: &mut ValidationErrors,
) {
    if !accept_non_blank(errors, "relay_agent_address", relay, "Relay agent address") {
        return;
    }
    if let Some(relay) = relay {
        errors.add_if_err("relay_agent_address", validate_ipv4_address(relay));
    }
    check_relay_uniqueness(conn, relay, exclude_network_id, errors).await;
}

/// Validate create network request using the generic validators
pub async fn validate_create_network_request(
    conn: &Connection,
    req: &CreateNetworkRequest,
) -> Result<(), HashMap<String, String>> {
    let mut errors = ValidationErrors::new();

    validate_network_name(conn, &req.name, None, &mut errors).await;

    // An explicit empty gateway is a client error; omit the field (or send null) for no gateway.
    let gateway = req.gateway.as_deref();
    let gateway_ok = accept_non_blank(&mut errors, "gateway", gateway, "Gateway");
    validate_subnet_and_gateway(
        &req.subnet,
        if gateway_ok { gateway } else { None },
        &mut errors,
    );

    // Empty DNS list is allowed -- it means "advertise no DNS servers".
    errors.add_if_err(
        "dns_servers",
        validate_ipv4_list(&req.dns_servers, "DNS server", 0),
    );
    errors.add_if_err(
        "lease_duration",
        validate_lease_duration(req.lease_duration),
    );

    validate_domain(&mut errors, req.domain_name.as_deref());
    validate_relay(conn, req.relay_agent_address.as_deref(), None, &mut errors).await;

    errors.into_result()
}

/// Validate update network request using the generic validators.
///
/// Excludes the network being updated from duplicate checks. `Patch` fields distinguish
/// "leave unchanged" from "clear": clearing the gateway removes it from subnet validation,
/// and clearing the relay agent turns the network into a local L2 network for the purposes
/// of the uniqueness check.
pub async fn validate_update_network_request(
    conn: &Connection,
    network_id: i64,
    req: &UpdateNetworkRequest,
) -> Result<(), HashMap<String, String>> {
    let mut errors = ValidationErrors::new();

    if let Some(ref name) = req.name {
        validate_network_name(conn, name, Some(network_id), &mut errors).await;
    }

    // A blank gateway is rejected here and then dropped from the co-validation so it cannot
    // pile a second error onto the same field; an accompanying subnet change is still checked.
    let gateway_value = req.gateway.value().map(String::as_str);
    let gateway_ok = accept_non_blank(&mut errors, "gateway", gateway_value, "Gateway");
    let gateway = match &req.gateway {
        Patch::Keep => None,
        Patch::Set(gw) if gateway_ok => Some(Some(gw.as_str())),
        Patch::Clear | Patch::Set(_) => Some(None),
    };
    validate_updated_subnet_and_gateway(
        conn,
        network_id,
        req.subnet.as_deref(),
        gateway,
        &mut errors,
    )
    .await;

    // Empty DNS list is allowed -- it means "advertise no DNS servers".
    if let Some(ref dns_servers) = req.dns_servers {
        errors.add_if_err(
            "dns_servers",
            validate_ipv4_list(dns_servers, "DNS server", 0),
        );
    }

    if let Some(lease_duration) = req.lease_duration {
        errors.add_if_err("lease_duration", validate_lease_duration(lease_duration));
    }

    validate_domain(&mut errors, req.domain_name.value().map(String::as_str));

    // `Keep` skips the check entirely; `Clear` is checked as a local L2 network.
    if !req.relay_agent_address.is_keep() {
        let relay = req.relay_agent_address.value().map(String::as_str);
        validate_relay(conn, relay, Some(network_id), &mut errors).await;
    }

    errors.into_result()
}

/// Validate the resulting subnet/gateway pair for an update, filling unchanged values in from
/// the stored network.
///
/// `subnet` is the new subnet, if changed. `gateway` is `None` to keep the stored gateway,
/// `Some(None)` when there is no gateway to check (cleared, or already rejected as blank), and
/// `Some(Some(g))` to check `g`. The stored network is only loaded when needed.
async fn validate_updated_subnet_and_gateway(
    conn: &Connection,
    network_id: i64,
    subnet: Option<&str>,
    gateway: Option<Option<&str>>,
    errors: &mut ValidationErrors,
) {
    if subnet.is_none() && !matches!(gateway, Some(Some(_))) {
        return;
    }

    let stored = if subnet.is_none() || gateway.is_none() {
        match dhcp::store::get_network(conn, network_id).await {
            Ok(net) => Some(net),
            Err(e) => {
                log::warn!("Failed to fetch current network for validation: {}", e);
                return;
            }
        }
    } else {
        None
    };

    let subnet = subnet.or_else(|| stored.as_ref().map(|n| n.subnet.as_str()));
    let gateway = gateway.unwrap_or_else(|| stored.as_ref().and_then(|n| n.gateway.as_deref()));

    if let Some(subnet) = subnet {
        validate_subnet_and_gateway(subnet, gateway, errors);
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use crate::{database::ConnectionFactory, test_connection_factory};

    use super::*;
    use std::sync::Arc;

    async fn create_test_store(factory: Arc<dyn ConnectionFactory>) -> Connection {
        crate::database::run_migrations(factory.as_ref())
            .await
            .unwrap()
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
    fn test_validate_mac_address() {
        assert!(validate_mac_address("aa:bb:cc:dd:ee:ff").is_none());
        assert!(validate_mac_address("AA:BB:CC:DD:EE:FF").is_none());
        assert!(validate_mac_address("aa:bb:cc:dd:ee").is_some());
        assert!(validate_mac_address("aa:bb:cc:dd:ee:gg").is_some());
        assert!(validate_mac_address("not-a-mac").is_some());
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
        let conn = create_test_store(Arc::new(test_connection_factory!())).await;
        let req = CreateNetworkRequest {
            name: "Test Network".to_string(),
            subnet: "192.168.1.0/24".to_string(),
            gateway: Some("192.168.1.1".to_string()),
            dns_servers: vec!["8.8.8.8".to_string()],
            lease_duration: 86400,
            relay_agent_address: Some("10.0.0.2".to_string()),
            enable_autodiscovery: false,
            domain_name: None,
        };

        assert!(validate_create_network_request(&conn, &req).await.is_ok());
    }

    #[tokio::test]
    async fn test_validate_create_network_request_no_gateway_no_dns() {
        // A network with no gateway and empty DNS list must be accepted.
        let conn = create_test_store(Arc::new(test_connection_factory!())).await;
        let req = CreateNetworkRequest {
            name: "Isolated Network".to_string(),
            subnet: "192.168.1.0/24".to_string(),
            gateway: None,
            dns_servers: vec![],
            lease_duration: 86400,
            relay_agent_address: Some("10.0.0.2".to_string()),
            enable_autodiscovery: false,
            domain_name: None,
        };

        assert!(validate_create_network_request(&conn, &req).await.is_ok());
    }

    #[tokio::test]
    async fn test_validate_create_network_request_empty_name() {
        let conn = create_test_store(Arc::new(test_connection_factory!())).await;
        let req = CreateNetworkRequest {
            name: "".to_string(),
            subnet: "192.168.1.0/24".to_string(),
            gateway: Some("192.168.1.1".to_string()),
            dns_servers: vec!["8.8.8.8".to_string()],
            lease_duration: 86400,
            relay_agent_address: Some("10.0.0.2".to_string()),
            enable_autodiscovery: false,
            domain_name: None,
        };

        let result = validate_create_network_request(&conn, &req).await;
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors.contains_key("name"));
    }

    #[tokio::test]
    async fn test_validate_create_network_request_duplicate_name() {
        let conn = create_test_store(Arc::new(test_connection_factory!())).await;

        // Create a network with a specific name
        dhcp::store::create_network(
            &conn,
            "Existing Network",
            "192.168.1.0/24",
            Some("192.168.1.1"),
            &["8.8.8.8".to_string()],
            86400,
            Some("10.0.0.2"),
            false,
            None,
        )
        .await
        .unwrap();

        // Try to create another network with the same name
        let req = CreateNetworkRequest {
            name: "Existing Network".to_string(),
            subnet: "192.168.2.0/24".to_string(),
            gateway: Some("192.168.2.1".to_string()),
            dns_servers: vec!["8.8.8.8".to_string()],
            lease_duration: 86400,
            relay_agent_address: Some("10.0.0.3".to_string()),
            enable_autodiscovery: false,
            domain_name: None,
        };

        let result = validate_create_network_request(&conn, &req).await;
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors.contains_key("name"));
        assert!(errors.get("name").unwrap().contains("already exists"));
    }

    #[tokio::test]
    async fn test_validate_create_network_request_duplicate_relay_agent() {
        let conn = create_test_store(Arc::new(test_connection_factory!())).await;

        // Create a network with a specific relay agent
        dhcp::store::create_network(
            &conn,
            "Network 1",
            "192.168.1.0/24",
            Some("192.168.1.1"),
            &["8.8.8.8".to_string()],
            86400,
            Some("10.0.0.2"),
            false,
            None,
        )
        .await
        .unwrap();

        // Try to create another network with the same relay agent
        let req = CreateNetworkRequest {
            name: "Network 2".to_string(),
            subnet: "192.168.2.0/24".to_string(),
            gateway: Some("192.168.2.1".to_string()),
            dns_servers: vec!["8.8.8.8".to_string()],
            lease_duration: 86400,
            relay_agent_address: Some("10.0.0.2".to_string()),
            enable_autodiscovery: false,
            domain_name: None,
        };

        let result = validate_create_network_request(&conn, &req).await;
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
    async fn test_validate_create_network_request_duplicate_local_l2() {
        let conn = create_test_store(Arc::new(test_connection_factory!())).await;

        // Create a local L2 network (no relay agent) first
        dhcp::store::create_network(
            &conn,
            "Local Network",
            "10.0.0.0/24",
            Some("10.0.0.1"),
            &["8.8.8.8".to_string()],
            86400,
            None,
            false,
            None,
        )
        .await
        .unwrap();

        // Try to create another local L2 network (no relay agent)
        let req = CreateNetworkRequest {
            name: "Another Local Network".to_string(),
            subnet: "192.168.2.0/24".to_string(),
            gateway: Some("192.168.2.1".to_string()),
            dns_servers: vec!["8.8.8.8".to_string()],
            lease_duration: 86400,
            relay_agent_address: None,
            enable_autodiscovery: false,
            domain_name: None,
        };

        let result = validate_create_network_request(&conn, &req).await;
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors.contains_key("relay_agent_address"));
        assert!(
            errors
                .get("relay_agent_address")
                .unwrap()
                .contains("Only one local L2 network is allowed")
        );
    }

    #[tokio::test]
    async fn test_validate_create_network_request_invalid_subnet() {
        let conn = create_test_store(Arc::new(test_connection_factory!())).await;
        let req = CreateNetworkRequest {
            name: "Test".to_string(),
            subnet: "invalid".to_string(),
            gateway: Some("192.168.1.1".to_string()),
            dns_servers: vec!["8.8.8.8".to_string()],
            lease_duration: 86400,
            relay_agent_address: Some("10.0.0.2".to_string()),
            enable_autodiscovery: false,
            domain_name: None,
        };

        let result = validate_create_network_request(&conn, &req).await;
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors.contains_key("subnet"));
    }

    #[tokio::test]
    async fn test_validate_create_network_request_gateway_out_of_subnet() {
        let conn = create_test_store(Arc::new(test_connection_factory!())).await;
        let req = CreateNetworkRequest {
            name: "Test".to_string(),
            subnet: "192.168.1.0/24".to_string(),
            gateway: Some("10.0.0.1".to_string()),
            dns_servers: vec!["8.8.8.8".to_string()],
            lease_duration: 86400,
            relay_agent_address: Some("10.0.0.2".to_string()),
            enable_autodiscovery: false,
            domain_name: None,
        };

        let result = validate_create_network_request(&conn, &req).await;
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors.contains_key("gateway"));
    }

    #[tokio::test]
    async fn test_validate_create_network_request_no_dns() {
        // Empty dns_servers is now valid — min count changed from 1 to 0.
        let conn = create_test_store(Arc::new(test_connection_factory!())).await;
        let req = CreateNetworkRequest {
            name: "Test".to_string(),
            subnet: "192.168.1.0/24".to_string(),
            gateway: Some("192.168.1.1".to_string()),
            dns_servers: vec![],
            lease_duration: 86400,
            relay_agent_address: Some("10.0.0.2".to_string()),
            enable_autodiscovery: false,
            domain_name: None,
        };

        assert!(validate_create_network_request(&conn, &req).await.is_ok());
    }

    #[tokio::test]
    async fn test_validate_create_network_request_invalid_lease_duration() {
        let conn = create_test_store(Arc::new(test_connection_factory!())).await;
        let req = CreateNetworkRequest {
            name: "Test".to_string(),
            subnet: "192.168.1.0/24".to_string(),
            gateway: Some("192.168.1.1".to_string()),
            dns_servers: vec!["8.8.8.8".to_string()],
            lease_duration: 0,
            relay_agent_address: Some("10.0.0.2".to_string()),
            enable_autodiscovery: false,
            domain_name: None,
        };

        let result = validate_create_network_request(&conn, &req).await;
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors.contains_key("lease_duration"));
    }

    #[tokio::test]
    async fn test_validate_create_network_request_invalid_relay() {
        let conn = create_test_store(Arc::new(test_connection_factory!())).await;
        let req = CreateNetworkRequest {
            name: "Test".to_string(),
            subnet: "192.168.1.0/24".to_string(),
            gateway: Some("192.168.1.1".to_string()),
            dns_servers: vec!["8.8.8.8".to_string()],
            lease_duration: 86400,
            relay_agent_address: Some("invalid".to_string()),
            enable_autodiscovery: false,
            domain_name: None,
        };

        let result = validate_create_network_request(&conn, &req).await;
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors.contains_key("relay_agent_address"));
    }

    #[tokio::test]
    async fn test_validate_create_network_request_multiple_errors() {
        let conn = create_test_store(Arc::new(test_connection_factory!())).await;
        let req = CreateNetworkRequest {
            name: "".to_string(),
            subnet: "invalid".to_string(),
            gateway: Some("10.0.0.1".to_string()),
            dns_servers: vec![],
            lease_duration: 0,
            relay_agent_address: None,
            enable_autodiscovery: false,
            domain_name: None,
        };

        let result = validate_create_network_request(&conn, &req).await;
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors.contains_key("name"));
        assert!(errors.contains_key("subnet"));
        // dns_servers is now allowed to be empty, so no error expected there
        assert!(errors.contains_key("lease_duration"));
    }

    // ========== Update Network Validation Tests ==========

    #[tokio::test]
    async fn test_validate_update_network_request_valid() {
        let conn = create_test_store(Arc::new(test_connection_factory!())).await;

        // Create a network to update
        let network = dhcp::store::create_network(
            &conn,
            "Original Network",
            "192.168.1.0/24",
            Some("192.168.1.1"),
            &["8.8.8.8".to_string()],
            86400,
            Some("10.0.0.2"),
            false,
            None,
        )
        .await
        .unwrap();

        let req = UpdateNetworkRequest {
            name: Some("Updated Network".to_string()),
            subnet: None,
            gateway: Patch::Keep,
            dns_servers: None,
            lease_duration: Some(7200),
            relay_agent_address: Patch::Keep,
            enable_autodiscovery: None,
            domain_name: Patch::Keep,
        };

        assert!(
            validate_update_network_request(&conn, network.id, &req)
                .await
                .is_ok()
        );
    }

    #[tokio::test]
    async fn test_validate_update_network_request_duplicate_name_different_network() {
        let conn = create_test_store(Arc::new(test_connection_factory!())).await;

        // Create two networks
        let network1 = dhcp::store::create_network(
            &conn,
            "Network 1",
            "192.168.1.0/24",
            Some("192.168.1.1"),
            &["8.8.8.8".to_string()],
            86400,
            Some("10.0.0.2"),
            false,
            None,
        )
        .await
        .unwrap();

        dhcp::store::create_network(
            &conn,
            "Network 2",
            "192.168.2.0/24",
            Some("192.168.2.1"),
            &["8.8.8.8".to_string()],
            86400,
            Some("10.0.0.3"),
            false,
            None,
        )
        .await
        .unwrap();

        // Try to update network1 to have the same name as network2
        let req = UpdateNetworkRequest {
            name: Some("Network 2".to_string()),
            subnet: None,
            gateway: Patch::Keep,
            dns_servers: None,
            lease_duration: None,
            relay_agent_address: Patch::Keep,
            enable_autodiscovery: None,
            domain_name: Patch::Keep,
        };

        let result = validate_update_network_request(&conn, network1.id, &req).await;
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
        let conn = create_test_store(Arc::new(test_connection_factory!())).await;

        // Create a network
        let network = dhcp::store::create_network(
            &conn,
            "Test Network",
            "192.168.1.0/24",
            Some("192.168.1.1"),
            &["8.8.8.8".to_string()],
            86400,
            Some("10.0.0.2"),
            false,
            None,
        )
        .await
        .unwrap();

        // Update with the same name (should be allowed)
        let req = UpdateNetworkRequest {
            name: Some("Test Network".to_string()),
            subnet: None,
            gateway: Patch::Keep,
            dns_servers: None,
            lease_duration: None,
            relay_agent_address: Patch::Keep,
            enable_autodiscovery: None,
            domain_name: Patch::Keep,
        };

        assert!(
            validate_update_network_request(&conn, network.id, &req)
                .await
                .is_ok()
        );
    }

    #[tokio::test]
    async fn test_validate_update_network_request_duplicate_relay_agent() {
        let conn = create_test_store(Arc::new(test_connection_factory!())).await;

        // Create two networks
        let network1 = dhcp::store::create_network(
            &conn,
            "Network 1",
            "192.168.1.0/24",
            Some("192.168.1.1"),
            &["8.8.8.8".to_string()],
            86400,
            Some("10.0.0.2"),
            false,
            None,
        )
        .await
        .unwrap();

        dhcp::store::create_network(
            &conn,
            "Network 2",
            "192.168.2.0/24",
            Some("192.168.2.1"),
            &["8.8.8.8".to_string()],
            86400,
            Some("10.0.0.3"),
            false,
            None,
        )
        .await
        .unwrap();

        // Try to update network1 to have the same relay agent as network2
        let req = UpdateNetworkRequest {
            name: None,
            subnet: None,
            gateway: Patch::Keep,
            dns_servers: None,
            lease_duration: None,
            relay_agent_address: Patch::Set("10.0.0.3".to_string()),
            enable_autodiscovery: None,
            domain_name: Patch::Keep,
        };

        let result = validate_update_network_request(&conn, network1.id, &req).await;
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors.contains_key("relay_agent_address"));
    }

    #[tokio::test]
    async fn test_validate_update_network_request_same_relay_agent_same_network() {
        let conn = create_test_store(Arc::new(test_connection_factory!())).await;

        // Create a network
        let network = dhcp::store::create_network(
            &conn,
            "Test Network",
            "192.168.1.0/24",
            Some("192.168.1.1"),
            &["8.8.8.8".to_string()],
            86400,
            Some("10.0.0.2"),
            false,
            None,
        )
        .await
        .unwrap();

        // Update with the same relay agent (should be allowed)
        let req = UpdateNetworkRequest {
            name: None,
            subnet: None,
            gateway: Patch::Keep,
            dns_servers: None,
            lease_duration: None,
            relay_agent_address: Patch::Set("10.0.0.2".to_string()),
            enable_autodiscovery: None,
            domain_name: Patch::Keep,
        };

        assert!(
            validate_update_network_request(&conn, network.id, &req)
                .await
                .is_ok()
        );
    }

    #[tokio::test]
    async fn test_validate_update_network_request_gateway_out_of_new_subnet() {
        let conn = create_test_store(Arc::new(test_connection_factory!())).await;

        // Create a network
        let network = dhcp::store::create_network(
            &conn,
            "Test Network",
            "192.168.1.0/24",
            Some("192.168.1.1"),
            &["8.8.8.8".to_string()],
            86400,
            Some("10.0.0.2"),
            false,
            None,
        )
        .await
        .unwrap();

        // Update subnet to a range that doesn't contain the gateway
        let req = UpdateNetworkRequest {
            name: None,
            subnet: Some("10.0.0.0/24".to_string()),
            gateway: Patch::Keep, // Will use existing gateway 192.168.1.1
            dns_servers: None,
            lease_duration: None,
            relay_agent_address: Patch::Keep,
            enable_autodiscovery: None,
            domain_name: Patch::Keep,
        };

        let result = validate_update_network_request(&conn, network.id, &req).await;
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors.contains_key("gateway"));
    }

    #[tokio::test]
    async fn test_validate_update_network_request_invalid_dns() {
        let conn = create_test_store(Arc::new(test_connection_factory!())).await;

        // Create a network
        let network = dhcp::store::create_network(
            &conn,
            "Test Network",
            "192.168.1.0/24",
            Some("192.168.1.1"),
            &["8.8.8.8".to_string()],
            86400,
            Some("10.0.0.2"),
            false,
            None,
        )
        .await
        .unwrap();

        // Update with invalid DNS
        let req = UpdateNetworkRequest {
            name: None,
            subnet: None,
            gateway: Patch::Keep,
            dns_servers: Some(vec!["invalid".to_string()]),
            lease_duration: None,
            relay_agent_address: Patch::Keep,
            enable_autodiscovery: None,
            domain_name: Patch::Keep,
        };

        let result = validate_update_network_request(&conn, network.id, &req).await;
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors.contains_key("dns_servers"));
    }

    // ========== Hostname Validation Tests ==========

    #[test]
    fn test_validate_hostname_valid() {
        assert!(validate_hostname("server-01").is_ok());
        assert!(validate_hostname("s").is_ok());
        assert!(validate_hostname("server123").is_ok());
        assert!(validate_hostname("my-server").is_ok());
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
        // 64 characters exceeds the RFC 1123 single-label limit of 63
        let long_hostname = "a".repeat(64);
        let result = validate_hostname(&long_hostname);
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors.contains_key("hostname"));
        assert!(
            errors
                .get("hostname")
                .unwrap()
                .contains("less than 63 characters")
        );
    }

    #[test]
    fn test_validate_hostname_max_length_valid() {
        // Exactly 63 characters should be valid
        let max_hostname = "a".repeat(63);
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
                .contains("only letters, numbers, and hyphens")
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

    /// A hostname must be a bare label — the domain is supplied by the device's network, so a
    /// dotted hostname would render a doubled domain in `{{ device.fqdn }}`.
    #[test]
    fn test_validate_hostname_dots_rejected() {
        for dotted in [
            "web.server",
            "app-server-01.example.com",
            "my-server.local",
            ".server",
            "server.",
            ".",
        ] {
            let errors = validate_hostname(dotted).expect_err(&format!(
                "{dotted} should be rejected as not a single label"
            ));
            assert!(
                errors
                    .get("hostname")
                    .unwrap()
                    .contains("must be a single label"),
                "unexpected message for {dotted}: {:?}",
                errors.get("hostname")
            );
        }
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

    // ========== Domain Name Validation Tests ==========

    #[test]
    fn test_validate_domain_name_valid() {
        assert!(validate_domain_name("test.local").is_none());
        assert!(validate_domain_name("a.b.example.com").is_none());
        assert!(validate_domain_name("local").is_none());
        assert!(validate_domain_name("my-domain.example.com").is_none());
    }

    #[test]
    fn test_validate_domain_name_leading_trailing_dot() {
        assert!(validate_domain_name(".example.com").is_some());
        assert!(validate_domain_name("example.com.").is_some());
    }

    #[test]
    fn test_validate_domain_name_empty_label() {
        assert!(validate_domain_name("a..b").is_some());
    }

    #[test]
    fn test_validate_domain_name_leading_trailing_hyphen() {
        assert!(validate_domain_name("-bad.local").is_some());
        assert!(validate_domain_name("bad-.local").is_some());
        assert!(validate_domain_name("test.-bad.local").is_some());
    }

    #[test]
    fn test_validate_domain_name_label_too_long() {
        let long_label = "a".repeat(64);
        let domain = format!("{}.local", long_label);
        assert!(validate_domain_name(&domain).is_some());

        // Exactly 63 characters is valid
        let max_label = "a".repeat(63);
        let domain = format!("{}.local", max_label);
        assert!(validate_domain_name(&domain).is_none());
    }

    #[test]
    fn test_validate_domain_name_too_long_total() {
        // Build a domain over 253 characters using valid labels
        let label = "a".repeat(50);
        let domain = vec![label; 6].join(".");
        assert!(domain.len() > 253);
        assert!(validate_domain_name(&domain).is_some());
    }

    #[test]
    fn test_validate_domain_name_invalid_characters() {
        assert!(validate_domain_name("bad_domain.local").is_some());
        assert!(validate_domain_name("bad domain.local").is_some());
        assert!(validate_domain_name("bad@domain.local").is_some());
        assert!(validate_domain_name("dömain.local").is_some());
    }

    // ========== Blank-value Rejection ==========

    #[test]
    fn test_validate_not_blank() {
        assert!(validate_not_blank(None, "Field").is_none());
        assert!(validate_not_blank(Some("value"), "Field").is_none());
        assert!(validate_not_blank(Some(""), "Field").is_some());
        assert!(validate_not_blank(Some("   "), "Field").is_some());
        assert!(
            validate_not_blank(Some(""), "Gateway")
                .unwrap()
                .contains("send null to clear")
        );
    }

    /// Build an update request whose patch fields default to `Keep`.
    fn blank_update_request() -> UpdateNetworkRequest {
        UpdateNetworkRequest::default()
    }

    async fn create_default_network(conn: &Connection) -> i64 {
        dhcp::store::create_network(
            conn,
            "Test Network",
            "192.168.1.0/24",
            Some("192.168.1.1"),
            &["8.8.8.8".to_string()],
            86400,
            Some("10.0.0.2"),
            false,
            Some("test.local"),
        )
        .await
        .unwrap()
        .id
    }

    #[tokio::test]
    async fn test_validate_update_network_request_empty_gateway_rejected() {
        let conn = create_test_store(Arc::new(test_connection_factory!())).await;
        let id = create_default_network(&conn).await;

        let req = UpdateNetworkRequest {
            gateway: Patch::Set(String::new()),
            ..blank_update_request()
        };

        let errors = validate_update_network_request(&conn, id, &req)
            .await
            .unwrap_err();
        assert!(errors.get("gateway").unwrap().contains("cannot be empty"));
    }

    /// A blank gateway must not mask an invalid subnet supplied in the same request.
    #[tokio::test]
    async fn test_validate_update_network_request_empty_gateway_still_checks_subnet() {
        let conn = create_test_store(Arc::new(test_connection_factory!())).await;
        let id = create_default_network(&conn).await;

        let req = UpdateNetworkRequest {
            subnet: Some("not-a-subnet".to_string()),
            gateway: Patch::Set(String::new()),
            ..blank_update_request()
        };

        let errors = validate_update_network_request(&conn, id, &req)
            .await
            .unwrap_err();
        assert!(errors.contains_key("subnet"));
        assert!(errors.get("gateway").unwrap().contains("cannot be empty"));
    }

    #[tokio::test]
    async fn test_validate_update_network_request_empty_relay_rejected() {
        let conn = create_test_store(Arc::new(test_connection_factory!())).await;
        let id = create_default_network(&conn).await;

        let req = UpdateNetworkRequest {
            relay_agent_address: Patch::Set(String::new()),
            ..blank_update_request()
        };

        let errors = validate_update_network_request(&conn, id, &req)
            .await
            .unwrap_err();
        assert!(
            errors
                .get("relay_agent_address")
                .unwrap()
                .contains("cannot be empty")
        );
    }

    #[tokio::test]
    async fn test_validate_update_network_request_empty_domain_rejected() {
        let conn = create_test_store(Arc::new(test_connection_factory!())).await;
        let id = create_default_network(&conn).await;

        let req = UpdateNetworkRequest {
            domain_name: Patch::Set(String::new()),
            ..blank_update_request()
        };

        let errors = validate_update_network_request(&conn, id, &req)
            .await
            .unwrap_err();
        assert!(
            errors
                .get("domain_name")
                .unwrap()
                .contains("cannot be empty")
        );
    }

    // ========== Clear Semantics ==========

    /// Clearing the gateway removes it from subnet co-validation — a network with no gateway
    /// is legal even when the subnet is simultaneously changed.
    #[tokio::test]
    async fn test_validate_update_network_request_clearing_gateway_skips_subnet_check() {
        let conn = create_test_store(Arc::new(test_connection_factory!())).await;
        let id = create_default_network(&conn).await;

        let req = UpdateNetworkRequest {
            subnet: Some("10.0.0.0/24".to_string()),
            gateway: Patch::Clear,
            ..blank_update_request()
        };

        // The stored gateway 192.168.1.1 is outside 10.0.0.0/24, but it is being cleared.
        assert!(
            validate_update_network_request(&conn, id, &req)
                .await
                .is_ok()
        );
    }

    /// Clearing the relay agent turns the network into a local L2 network, so it collides with
    /// an existing relay-less network.
    #[tokio::test]
    async fn test_validate_update_network_request_clearing_relay_hits_local_l2_rule() {
        let conn = create_test_store(Arc::new(test_connection_factory!())).await;
        let id = create_default_network(&conn).await;

        dhcp::store::create_network(
            &conn,
            "Local Network",
            "10.0.0.0/24",
            Some("10.0.0.1"),
            &[],
            86400,
            None,
            false,
            None,
        )
        .await
        .unwrap();

        let req = UpdateNetworkRequest {
            relay_agent_address: Patch::Clear,
            ..blank_update_request()
        };

        let errors = validate_update_network_request(&conn, id, &req)
            .await
            .unwrap_err();
        assert!(
            errors
                .get("relay_agent_address")
                .unwrap()
                .contains("Only one local L2 network is allowed")
        );
    }

    /// Clearing the relay agent is allowed when no other relay-less network exists.
    #[tokio::test]
    async fn test_validate_update_network_request_clearing_relay_allowed_when_unique() {
        let conn = create_test_store(Arc::new(test_connection_factory!())).await;
        let id = create_default_network(&conn).await;

        let req = UpdateNetworkRequest {
            relay_agent_address: Patch::Clear,
            ..blank_update_request()
        };

        assert!(
            validate_update_network_request(&conn, id, &req)
                .await
                .is_ok()
        );
    }

    /// Keeping the relay agent must not run the duplicate check at all.
    #[tokio::test]
    async fn test_validate_update_network_request_keeping_relay_skips_check() {
        let conn = create_test_store(Arc::new(test_connection_factory!())).await;
        let id = create_default_network(&conn).await;

        dhcp::store::create_network(
            &conn,
            "Local Network",
            "10.0.0.0/24",
            Some("10.0.0.1"),
            &[],
            86400,
            None,
            false,
            None,
        )
        .await
        .unwrap();

        assert!(
            validate_update_network_request(&conn, id, &blank_update_request())
                .await
                .is_ok()
        );
    }

    // ========== Create-path Blank Rejection ==========

    fn create_request_with(
        gateway: Option<&str>,
        relay: Option<&str>,
        domain: Option<&str>,
    ) -> CreateNetworkRequest {
        CreateNetworkRequest {
            name: "Test".to_string(),
            subnet: "192.168.1.0/24".to_string(),
            gateway: gateway.map(str::to_string),
            dns_servers: vec![],
            lease_duration: 86400,
            relay_agent_address: relay.map(str::to_string),
            enable_autodiscovery: false,
            domain_name: domain.map(str::to_string),
        }
    }

    #[tokio::test]
    async fn test_validate_create_network_request_empty_gateway_rejected() {
        let conn = create_test_store(Arc::new(test_connection_factory!())).await;
        let req = create_request_with(Some(""), Some("10.0.0.2"), None);

        let errors = validate_create_network_request(&conn, &req)
            .await
            .unwrap_err();
        assert!(errors.get("gateway").unwrap().contains("cannot be empty"));
    }

    #[tokio::test]
    async fn test_validate_create_network_request_empty_relay_rejected() {
        let conn = create_test_store(Arc::new(test_connection_factory!())).await;
        let req = create_request_with(Some("192.168.1.1"), Some(""), None);

        let errors = validate_create_network_request(&conn, &req)
            .await
            .unwrap_err();
        assert!(
            errors
                .get("relay_agent_address")
                .unwrap()
                .contains("cannot be empty")
        );
    }

    /// The consistency bug being fixed: `domain_name: ""` used to be persisted as `''` on
    /// create while the update path silently mapped it to NULL.
    #[tokio::test]
    async fn test_validate_create_network_request_empty_domain_rejected() {
        let conn = create_test_store(Arc::new(test_connection_factory!())).await;
        let req = create_request_with(Some("192.168.1.1"), Some("10.0.0.2"), Some(""));

        let errors = validate_create_network_request(&conn, &req)
            .await
            .unwrap_err();
        assert!(
            errors
                .get("domain_name")
                .unwrap()
                .contains("cannot be empty")
        );
    }

    #[tokio::test]
    async fn test_validate_create_network_request_absent_optionals_ok() {
        let conn = create_test_store(Arc::new(test_connection_factory!())).await;
        let req = create_request_with(None, Some("10.0.0.2"), None);

        assert!(validate_create_network_request(&conn, &req).await.is_ok());
    }
}
