use super::store::DhcpNetwork;
use common::Ipv4Subnet;
use network_interface::{Addr, NetworkInterface, NetworkInterfaceConfig};
use std::net::Ipv4Addr;

/// Returns all IPv4 addresses assigned to the interface with the given index.
pub fn get_ipv4_addresses_for_interface(if_index: u32) -> anyhow::Result<Vec<Ipv4Addr>> {
    let interfaces = NetworkInterface::show()?;
    let addrs = interfaces
        .into_iter()
        .filter(|iface| iface.index == if_index)
        .flat_map(|iface| iface.addr)
        .filter_map(|addr| match addr {
            Addr::V4(v4) => Some(v4.ip),
            _ => None,
        })
        .collect();
    Ok(addrs)
}

/// Given a list of L2 networks, find which one's subnet contains any IPv4 address on the
/// given interface. Returns the matched network and the local IP that matched.
///
/// The local IP is used as the source IP for sending (so replies egress on the correct interface)
/// and as the DHCP Server Identifier (Option 54) in the reply.
pub fn find_matching_l2_network(
    if_index: u32,
    networks: &[DhcpNetwork],
) -> anyhow::Result<Option<(&DhcpNetwork, Ipv4Addr)>> {
    let local_ips = get_ipv4_addresses_for_interface(if_index)?;
    for network in networks {
        let subnet: Ipv4Subnet = network
            .subnet
            .parse()
            .map_err(|e: common::Ipv4SubnetError| anyhow::anyhow!("{}", e))?;
        for &ip in &local_ips {
            if subnet.ip_in_range(ip) {
                return Ok(Some((network, ip)));
            }
        }
    }
    Ok(None)
}

/// Given a local IP, find which L2 network's subnet contains it.
///
/// Used by per-network receive loops where the local IP is known from the socket bind address.
pub fn find_l2_network_for_ip(
    local_ip: Ipv4Addr,
    networks: &[DhcpNetwork],
) -> anyhow::Result<Option<&DhcpNetwork>> {
    for network in networks {
        let subnet: Ipv4Subnet = network
            .subnet
            .parse()
            .map_err(|e: common::Ipv4SubnetError| anyhow::anyhow!("{}", e))?;
        if subnet.ip_in_range(local_ip) {
            return Ok(Some(network));
        }
    }
    Ok(None)
}

/// Find the local interface IP that belongs to `subnet`.
///
/// Returns `Ok(None)` when no local interface has an address within the
/// subnet (e.g. relay-only network). Returns `Err` on OS-level failures.
pub fn find_local_ip_for_subnet(subnet: &str) -> anyhow::Result<Option<Ipv4Addr>> {
    let subnet: common::Ipv4Subnet = subnet
        .parse()
        .map_err(|e: common::Ipv4SubnetError| anyhow::anyhow!("{}", e))?;

    let all_ifaces = NetworkInterface::show()?;
    let local_ip = all_ifaces
        .iter()
        .flat_map(|iface| &iface.addr)
        .filter_map(|addr| match addr {
            Addr::V4(v4) => Some(v4.ip),
            _ => None,
        })
        .find(|&ip| subnet.ip_in_range(ip));

    Ok(local_ip)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn loopback_index() -> u32 {
        // Find the loopback interface index
        NetworkInterface::show()
            .unwrap()
            .into_iter()
            .find(|iface| {
                iface
                    .addr
                    .iter()
                    .any(|addr| matches!(addr, Addr::V4(v4) if v4.ip == Ipv4Addr::LOCALHOST))
            })
            .map(|iface| iface.index)
            .expect("loopback interface not found")
    }

    fn make_l2_network(subnet: &str) -> DhcpNetwork {
        use chrono::Utc;
        DhcpNetwork {
            id: 1,
            name: "test".to_string(),
            subnet: subnet.to_string(),
            gateway: "127.0.0.1".to_string(),
            dns_servers: vec![],
            lease_duration: 3600,
            relay_agent_address: None,
            enable_autodiscovery: false,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn test_get_ipv4_for_loopback() {
        let idx = loopback_index();
        let addrs = get_ipv4_addresses_for_interface(idx).unwrap();
        assert!(
            addrs.contains(&Ipv4Addr::LOCALHOST),
            "loopback must have 127.0.0.1"
        );
    }

    #[test]
    fn test_find_matching_l2_network_match() {
        let idx = loopback_index();
        let network = make_l2_network("127.0.0.0/8");
        let networks = vec![network];
        let result = find_matching_l2_network(idx, &networks).unwrap();
        assert!(result.is_some());
        let (_, local_ip) = result.unwrap();
        assert_eq!(local_ip, Ipv4Addr::LOCALHOST);
    }

    #[test]
    fn test_find_matching_l2_network_no_match() {
        let idx = loopback_index();
        let network = make_l2_network("192.168.100.0/24");
        let networks = vec![network];
        let result = find_matching_l2_network(idx, &networks).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_find_l2_network_for_ip() {
        let network = make_l2_network("127.0.0.0/8");
        let networks = vec![network];
        let result = find_l2_network_for_ip(Ipv4Addr::LOCALHOST, &networks).unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap().subnet, "127.0.0.0/8");
    }

    #[test]
    fn test_find_l2_network_for_ip_no_match() {
        let network = make_l2_network("192.168.100.0/24");
        let networks = vec![network];
        let result = find_l2_network_for_ip(Ipv4Addr::LOCALHOST, &networks).unwrap();
        assert!(result.is_none());
    }
}
