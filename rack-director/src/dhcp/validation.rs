use std::fmt;
use std::net::Ipv4Addr;

/// Errors that can occur during IP validation
#[derive(Debug, PartialEq)]
pub enum ValidationError {
    InvalidCidr(String),
    InvalidPrefixLength(u8),
    InvalidIp(String),
    IpNotInSubnet { ip: String, subnet: String },
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ValidationError::InvalidCidr(subnet) => {
                write!(
                    f,
                    "Invalid CIDR format: {}. Expected format like '10.0.0.0/24'",
                    subnet
                )
            }
            ValidationError::InvalidPrefixLength(len) => {
                write!(f, "Invalid prefix length: {}. Must be between 0 and 32", len)
            }
            ValidationError::InvalidIp(ip) => {
                write!(f, "Invalid IP address: {}", ip)
            }
            ValidationError::IpNotInSubnet { ip, subnet } => {
                write!(f, "IP address {} is not in subnet {}", ip, subnet)
            }
        }
    }
}

impl std::error::Error for ValidationError {}

/// Parse CIDR notation into network address and prefix length
///
/// # Arguments
/// * `subnet` - CIDR notation string (e.g., "10.0.0.0/24")
///
/// # Returns
/// * `Ok((network_address, prefix_length))` on success
/// * `Err(ValidationError)` if the format is invalid
pub fn parse_cidr(subnet: &str) -> Result<(Ipv4Addr, u8), ValidationError> {
    let parts: Vec<&str> = subnet.split('/').collect();
    if parts.len() != 2 {
        return Err(ValidationError::InvalidCidr(subnet.to_string()));
    }

    let network_addr = parts[0]
        .parse::<Ipv4Addr>()
        .map_err(|_| ValidationError::InvalidCidr(subnet.to_string()))?;

    let prefix_len: u8 = parts[1]
        .parse()
        .map_err(|_| ValidationError::InvalidCidr(subnet.to_string()))?;

    if prefix_len > 32 {
        return Err(ValidationError::InvalidPrefixLength(prefix_len));
    }

    Ok((network_addr, prefix_len))
}

/// Check if an IP address is within a subnet
///
/// # Arguments
/// * `ip` - The IP address to check
/// * `network` - The network address
/// * `prefix_len` - The subnet prefix length (0-32)
///
/// # Returns
/// * `true` if the IP is within the subnet, `false` otherwise
pub fn is_ip_in_subnet(ip: &Ipv4Addr, network: &Ipv4Addr, prefix_len: u8) -> bool {
    if prefix_len > 32 {
        return false;
    }

    // Calculate the subnet mask
    let mask = if prefix_len == 0 {
        0u32
    } else {
        !0u32 << (32 - prefix_len)
    };

    let ip_u32: u32 = (*ip).into();
    let network_u32: u32 = (*network).into();

    // Check if the network portions match
    (ip_u32 & mask) == (network_u32 & mask)
}

/// Validate that an IP address is within a subnet
///
/// This is a convenience function that combines parsing and validation.
///
/// # Arguments
/// * `ip` - IP address string to validate
/// * `subnet` - Subnet in CIDR notation (e.g., "10.0.0.0/24")
///
/// # Returns
/// * `Ok(())` if the IP is valid and within the subnet
/// * `Err(ValidationError)` with a descriptive error otherwise
pub fn validate_ip_in_network(ip: &str, subnet: &str) -> Result<(), ValidationError> {
    // Parse the IP address
    let ip_addr = ip
        .parse::<Ipv4Addr>()
        .map_err(|_| ValidationError::InvalidIp(ip.to_string()))?;

    // Parse the CIDR notation
    let (network_addr, prefix_len) = parse_cidr(subnet)?;

    // Check if IP is in subnet
    if is_ip_in_subnet(&ip_addr, &network_addr, prefix_len) {
        Ok(())
    } else {
        Err(ValidationError::IpNotInSubnet {
            ip: ip.to_string(),
            subnet: subnet.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========== parse_cidr tests ==========

    #[test]
    fn test_parse_cidr_valid() {
        let (network, prefix) = parse_cidr("10.0.0.0/24").unwrap();
        assert_eq!(network, Ipv4Addr::new(10, 0, 0, 0));
        assert_eq!(prefix, 24);
    }

    #[test]
    fn test_parse_cidr_valid_class_a() {
        let (network, prefix) = parse_cidr("192.168.1.0/16").unwrap();
        assert_eq!(network, Ipv4Addr::new(192, 168, 1, 0));
        assert_eq!(prefix, 16);
    }

    #[test]
    fn test_parse_cidr_valid_slash_32() {
        let (network, prefix) = parse_cidr("10.0.0.1/32").unwrap();
        assert_eq!(network, Ipv4Addr::new(10, 0, 0, 1));
        assert_eq!(prefix, 32);
    }

    #[test]
    fn test_parse_cidr_valid_slash_0() {
        let (network, prefix) = parse_cidr("0.0.0.0/0").unwrap();
        assert_eq!(network, Ipv4Addr::new(0, 0, 0, 0));
        assert_eq!(prefix, 0);
    }

    #[test]
    fn test_parse_cidr_invalid_no_slash() {
        let result = parse_cidr("10.0.0.0");
        assert!(matches!(result, Err(ValidationError::InvalidCidr(_))));
    }

    #[test]
    fn test_parse_cidr_invalid_too_many_slashes() {
        let result = parse_cidr("10.0.0.0/24/16");
        assert!(matches!(result, Err(ValidationError::InvalidCidr(_))));
    }

    #[test]
    fn test_parse_cidr_invalid_ip() {
        let result = parse_cidr("999.999.999.999/24");
        assert!(matches!(result, Err(ValidationError::InvalidCidr(_))));
    }

    #[test]
    fn test_parse_cidr_invalid_prefix_not_number() {
        let result = parse_cidr("10.0.0.0/abc");
        assert!(matches!(result, Err(ValidationError::InvalidCidr(_))));
    }

    #[test]
    fn test_parse_cidr_invalid_prefix_too_large() {
        let result = parse_cidr("10.0.0.0/33");
        assert_eq!(result, Err(ValidationError::InvalidPrefixLength(33)));
    }

    #[test]
    fn test_parse_cidr_invalid_prefix_way_too_large() {
        let result = parse_cidr("10.0.0.0/255");
        assert_eq!(result, Err(ValidationError::InvalidPrefixLength(255)));
    }

    // ========== is_ip_in_subnet tests ==========

    #[test]
    fn test_is_ip_in_subnet_slash_24() {
        let network = Ipv4Addr::new(10, 0, 0, 0);

        // IPs within subnet
        assert!(is_ip_in_subnet(
            &Ipv4Addr::new(10, 0, 0, 1),
            &network,
            24
        ));
        assert!(is_ip_in_subnet(
            &Ipv4Addr::new(10, 0, 0, 100),
            &network,
            24
        ));
        assert!(is_ip_in_subnet(
            &Ipv4Addr::new(10, 0, 0, 254),
            &network,
            24
        ));
        assert!(is_ip_in_subnet(
            &Ipv4Addr::new(10, 0, 0, 255),
            &network,
            24
        ));

        // IPs outside subnet
        assert!(!is_ip_in_subnet(
            &Ipv4Addr::new(10, 0, 1, 0),
            &network,
            24
        ));
        assert!(!is_ip_in_subnet(
            &Ipv4Addr::new(10, 0, 1, 100),
            &network,
            24
        ));
        assert!(!is_ip_in_subnet(
            &Ipv4Addr::new(11, 0, 0, 0),
            &network,
            24
        ));
    }

    #[test]
    fn test_is_ip_in_subnet_slash_16() {
        let network = Ipv4Addr::new(192, 168, 0, 0);

        // IPs within subnet
        assert!(is_ip_in_subnet(
            &Ipv4Addr::new(192, 168, 1, 1),
            &network,
            16
        ));
        assert!(is_ip_in_subnet(
            &Ipv4Addr::new(192, 168, 255, 255),
            &network,
            16
        ));

        // IPs outside subnet
        assert!(!is_ip_in_subnet(
            &Ipv4Addr::new(192, 169, 0, 0),
            &network,
            16
        ));
        assert!(!is_ip_in_subnet(
            &Ipv4Addr::new(10, 0, 0, 1),
            &network,
            16
        ));
    }

    #[test]
    fn test_is_ip_in_subnet_slash_8() {
        let network = Ipv4Addr::new(10, 0, 0, 0);

        // IPs within subnet
        assert!(is_ip_in_subnet(
            &Ipv4Addr::new(10, 255, 255, 255),
            &network,
            8
        ));
        assert!(is_ip_in_subnet(
            &Ipv4Addr::new(10, 1, 2, 3),
            &network,
            8
        ));

        // IPs outside subnet
        assert!(!is_ip_in_subnet(
            &Ipv4Addr::new(11, 0, 0, 0),
            &network,
            8
        ));
    }

    #[test]
    fn test_is_ip_in_subnet_slash_32() {
        let network = Ipv4Addr::new(10, 0, 0, 1);

        // Only exact IP should match
        assert!(is_ip_in_subnet(
            &Ipv4Addr::new(10, 0, 0, 1),
            &network,
            32
        ));

        // Even one bit off should fail
        assert!(!is_ip_in_subnet(
            &Ipv4Addr::new(10, 0, 0, 2),
            &network,
            32
        ));
        assert!(!is_ip_in_subnet(
            &Ipv4Addr::new(10, 0, 0, 0),
            &network,
            32
        ));
    }

    #[test]
    fn test_is_ip_in_subnet_slash_0() {
        let network = Ipv4Addr::new(0, 0, 0, 0);

        // Everything should match /0
        assert!(is_ip_in_subnet(
            &Ipv4Addr::new(0, 0, 0, 0),
            &network,
            0
        ));
        assert!(is_ip_in_subnet(
            &Ipv4Addr::new(255, 255, 255, 255),
            &network,
            0
        ));
        assert!(is_ip_in_subnet(
            &Ipv4Addr::new(192, 168, 1, 1),
            &network,
            0
        ));
    }

    #[test]
    fn test_is_ip_in_subnet_edge_case_slash_31() {
        let network = Ipv4Addr::new(10, 0, 0, 0);

        // /31 networks have only 2 addresses
        assert!(is_ip_in_subnet(
            &Ipv4Addr::new(10, 0, 0, 0),
            &network,
            31
        ));
        assert!(is_ip_in_subnet(
            &Ipv4Addr::new(10, 0, 0, 1),
            &network,
            31
        ));
        assert!(!is_ip_in_subnet(
            &Ipv4Addr::new(10, 0, 0, 2),
            &network,
            31
        ));
    }

    #[test]
    fn test_is_ip_in_subnet_invalid_prefix() {
        let network = Ipv4Addr::new(10, 0, 0, 0);
        let ip = Ipv4Addr::new(10, 0, 0, 1);

        // Prefix > 32 should return false
        assert!(!is_ip_in_subnet(&ip, &network, 33));
        assert!(!is_ip_in_subnet(&ip, &network, 255));
    }

    // ========== validate_ip_in_network tests ==========

    #[test]
    fn test_validate_ip_in_network_valid() {
        assert!(validate_ip_in_network("10.0.0.100", "10.0.0.0/24").is_ok());
        assert!(validate_ip_in_network("192.168.1.50", "192.168.0.0/16").is_ok());
    }

    #[test]
    fn test_validate_ip_in_network_ip_outside_subnet() {
        let result = validate_ip_in_network("10.0.1.100", "10.0.0.0/24");
        assert!(matches!(
            result,
            Err(ValidationError::IpNotInSubnet { .. })
        ));
    }

    #[test]
    fn test_validate_ip_in_network_invalid_ip() {
        let result = validate_ip_in_network("999.999.999.999", "10.0.0.0/24");
        assert!(matches!(result, Err(ValidationError::InvalidIp(_))));
    }

    #[test]
    fn test_validate_ip_in_network_invalid_subnet() {
        let result = validate_ip_in_network("10.0.0.100", "10.0.0.0");
        assert!(matches!(result, Err(ValidationError::InvalidCidr(_))));
    }

    #[test]
    fn test_validate_ip_in_network_invalid_prefix() {
        let result = validate_ip_in_network("10.0.0.100", "10.0.0.0/33");
        assert!(matches!(
            result,
            Err(ValidationError::InvalidPrefixLength(_))
        ));
    }

    #[test]
    fn test_validate_ip_in_network_boundary_cases() {
        // Network address itself
        assert!(validate_ip_in_network("10.0.0.0", "10.0.0.0/24").is_ok());
        // Broadcast address
        assert!(validate_ip_in_network("10.0.0.255", "10.0.0.0/24").is_ok());
        // First usable
        assert!(validate_ip_in_network("10.0.0.1", "10.0.0.0/24").is_ok());
        // Last usable
        assert!(validate_ip_in_network("10.0.0.254", "10.0.0.0/24").is_ok());
    }

    #[test]
    fn test_validate_ip_in_network_complex_subnets() {
        // Test with /28 (16 addresses)
        assert!(validate_ip_in_network("192.168.1.16", "192.168.1.16/28").is_ok());
        assert!(validate_ip_in_network("192.168.1.31", "192.168.1.16/28").is_ok());
        assert!(validate_ip_in_network("192.168.1.32", "192.168.1.16/28").is_err());

        // Test with /25 (128 addresses)
        assert!(validate_ip_in_network("10.0.0.127", "10.0.0.0/25").is_ok());
        assert!(validate_ip_in_network("10.0.0.128", "10.0.0.0/25").is_err());
    }
}
