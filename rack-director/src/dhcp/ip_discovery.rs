//! IP address discovery module for determining the server's outbound IP address.
//!
//! This module provides functionality to automatically discover the IP address
//! that this server would use for outbound connections. This is particularly
//! useful for determining the DHCP Server Identifier (Option 54) when not
//! explicitly configured.
//!
//! # Implementation
//!
//! The discovery uses a socket binding trick: we bind a UDP socket to 0.0.0.0:0
//! (any interface, any port) and then "connect" it to a public IP address
//! (8.8.8.8:80). The connect() call doesn't actually send any packets for UDP,
//! but it does cause the kernel to select the appropriate outbound interface
//! and assign a local address. We can then query the socket's local address
//! to determine which IP address would be used for outbound connections.

use anyhow::Result;
use common::Ipv4Subnet;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket};

/// Discovers the server's outbound IP address by binding a UDP socket
/// and connecting to a public IP address.
///
/// This function uses a socket binding trick to determine which IP address
/// the server would use for outbound connections. It binds to 0.0.0.0:0
/// (any interface, any port) and connects to 8.8.8.8:80 (Google's public DNS).
/// The connect() call for UDP doesn't actually send packets, but it causes
/// the kernel to select the appropriate outbound interface and assign a
/// local address.
///
/// # Returns
///
/// Returns the discovered IPv4 address on success, or an error if:
/// - Socket creation fails
/// - The connect operation fails
/// - Getting the local address fails
/// - The local address is not IPv4
///
/// # Example
///
/// ```ignore
/// // This is an internal module, so we can't access it from doctests
/// // See the unit tests for actual usage examples
/// use rack_director::dhcp::discover_server_identifier;
///
/// let server_ip = discover_server_identifier().unwrap();
/// println!("Server IP: {}", server_ip);
/// ```
pub fn discover_server_identifier() -> Result<Ipv4Addr> {
    discover_outgoing_ip_for(SocketAddr::new(Ipv4Addr::new(8, 8, 8, 8).into(), 80))
}

/// Guess if the given subnet is a local network.
pub fn is_subnet_local(subnet: Ipv4Subnet) -> Result<bool> {
    let local_addr = discover_outgoing_ip_for(SocketAddrV4::new(subnet.addr, 80).into())?;
    Ok(subnet.ip_in_range(local_addr))
}

fn discover_outgoing_ip_for(addr: SocketAddr) -> Result<Ipv4Addr> {
    // Bind to any interface on any port
    let socket = UdpSocket::bind(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0))?;

    // This doesn't send any packets for UDP, but causes the kernel to select
    // the appropriate outbound interface and assign a local address.
    socket.connect(addr)?;

    // Get the local address assigned by the kernel
    let local_addr = socket.local_addr()?;

    // Extract the IPv4 address
    match local_addr {
        SocketAddr::V4(addr) => Ok(*addr.ip()),
        SocketAddr::V6(_) => Err(anyhow::anyhow!(
            "Expected IPv4 address but got IPv6: {}",
            local_addr
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_discover_server_identifier() {
        // Test that discovery returns a valid IPv4 address
        let result = discover_server_identifier();
        assert!(result.is_ok(), "Discovery should succeed");

        let ip = result.unwrap();

        // Verify we got a valid IPv4 address (not unspecified)
        assert_ne!(
            ip,
            Ipv4Addr::UNSPECIFIED,
            "Should not return unspecified address (0.0.0.0)"
        );

        // Log the discovered IP for debugging
        println!("Discovered server IP: {}", ip);

        // The IP should be either a private network address or a public address
        // We can't assert specific values since it depends on the test environment,
        // but we can verify it's not localhost
        assert_ne!(
            ip,
            Ipv4Addr::LOCALHOST,
            "Should not return localhost (127.0.0.1)"
        );
    }

    #[test]
    fn test_discover_returns_ipv4() {
        // Verify the result is specifically IPv4, not IPv6
        let result = discover_server_identifier();
        assert!(result.is_ok());

        let ip = result.unwrap();
        // Ipv4Addr type ensures this is IPv4, but let's verify it's valid
        assert!(ip.octets().len() == 4);
    }

    #[test]
    fn test_discover_is_deterministic() {
        // Discovery should return the same IP on repeated calls
        let ip1 = discover_server_identifier().unwrap();
        let ip2 = discover_server_identifier().unwrap();
        assert_eq!(
            ip1, ip2,
            "Repeated discovery calls should return the same IP"
        );
    }
}
