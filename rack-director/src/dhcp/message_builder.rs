use anyhow::Result;
use common::Ipv4Subnet;
use dhcproto::v4::{self, Message, MessageType, Opcode};
use std::net::Ipv4Addr;

use super::store::DhcpNetwork;

/// Creates a base DHCP reply message with common fields copied from the request.
///
/// This function initializes a reply message with:
/// - Opcode set to BootReply
/// - Transaction ID (xid) copied from request
/// - Client hardware address (chaddr) copied from request
/// - Flags copied from request
///
/// # Arguments
/// * `req` - The incoming DHCP request message
///
/// # Returns
/// A new `Message` initialized with base reply fields
pub fn create_base_reply(req: &Message, siaddr: &Ipv4Addr) -> Message {
    let mut msg = Message::default();
    msg.set_opcode(Opcode::BootReply);
    msg.set_xid(req.xid());
    msg.set_chaddr(req.chaddr());
    msg.set_siaddr(siaddr.clone());
    msg.set_flags(req.flags());
    msg
}

/// Adds network configuration options to a DHCP message.
///
/// This function adds the following DHCP options:
/// - Option 1: Subnet Mask (calculated from network subnet CIDR)
/// - Option 3: Router (gateway address)
/// - Option 6: DNS Servers (if configured)
///
/// # Arguments
/// * `msg` - The DHCP message to modify
/// * `network` - The network configuration containing subnet, gateway, and DNS settings
///
/// # Returns
/// * `Ok(())` - Options added successfully
/// * `Err(_)` - Failed to parse network configuration
pub fn add_network_options(msg: &mut Message, network: &DhcpNetwork) -> Result<()> {
    let subnet: Ipv4Subnet = network.subnet.parse()?;
    msg.opts_mut()
        .insert(v4::DhcpOption::SubnetMask(subnet.netmask()));
    msg.opts_mut()
        .insert(v4::DhcpOption::Router(vec![network.gateway.parse()?]));

    let dns_servers: Vec<Ipv4Addr> = network
        .dns_servers
        .iter()
        .filter_map(|s| s.parse().ok())
        .collect();
    if !dns_servers.is_empty() {
        msg.opts_mut()
            .insert(v4::DhcpOption::DomainNameServer(dns_servers));
    }

    Ok(())
}

/// Builds a DHCP NAK message.
///
/// A NAK (Negative Acknowledgement) is sent when the server cannot fulfill a
/// DHCP REQUEST (e.g., requested IP doesn't match offered IP, or lease not found).
///
/// The NAK message includes:
/// - Message Type: NAK
/// - Server Identifier
/// - Base reply fields (xid, chaddr, flags)
///
/// # Arguments
/// * `req` - The incoming DHCP request message
/// * `server_identifier` - The DHCP server's identifier IP address
///
/// # Returns
/// A DHCP NAK message ready to send
pub fn build_nak(req: &Message, server_identifier: Ipv4Addr) -> Message {
    let mut msg = create_base_reply(req, &server_identifier);

    msg.opts_mut()
        .insert(v4::DhcpOption::MessageType(MessageType::Nak));

    msg.opts_mut()
        .insert(v4::DhcpOption::ServerIdentifier(server_identifier));

    msg
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_base_reply() {
        use dhcproto::v4::Flags;

        let mut req = Message::default();
        req.set_opcode(Opcode::BootRequest);
        req.set_xid(0x12345678);
        req.set_chaddr(&[0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff]);
        req.set_flags(Flags::default().set_broadcast());

        let reply = create_base_reply(&req, &Ipv4Addr::LOCALHOST);

        assert_eq!(reply.opcode(), Opcode::BootReply);
        assert_eq!(reply.xid(), 0x12345678);
        assert_eq!(reply.chaddr(), &[0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff]);
        assert_eq!(reply.flags(), Flags::default().set_broadcast());
    }

    #[test]
    fn test_add_network_options() {
        use chrono::Utc;

        let network = DhcpNetwork {
            id: 1,
            name: "test-network".to_string(),
            subnet: "10.0.0.0/24".to_string(),
            gateway: "10.0.0.1".to_string(),
            dns_servers: vec!["8.8.8.8".to_string(), "8.8.4.4".to_string()],
            lease_duration: 3600,
            relay_agent_address: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        let mut msg = Message::default();
        add_network_options(&mut msg, &network).unwrap();

        // Verify subnet mask
        let subnet_mask = msg
            .opts()
            .iter()
            .find_map(|(_, opt)| {
                if let v4::DhcpOption::SubnetMask(mask) = opt {
                    Some(*mask)
                } else {
                    None
                }
            })
            .expect("SubnetMask should be present");
        assert_eq!(subnet_mask.to_string(), "255.255.255.0");

        // Verify router
        let router = msg
            .opts()
            .iter()
            .find_map(|(_, opt)| {
                if let v4::DhcpOption::Router(routers) = opt {
                    Some(routers.clone())
                } else {
                    None
                }
            })
            .expect("Router should be present");
        assert_eq!(router.len(), 1);
        assert_eq!(router[0].to_string(), "10.0.0.1");

        // Verify DNS servers
        let dns = msg
            .opts()
            .iter()
            .find_map(|(_, opt)| {
                if let v4::DhcpOption::DomainNameServer(servers) = opt {
                    Some(servers.clone())
                } else {
                    None
                }
            })
            .expect("DNS servers should be present");
        assert_eq!(dns.len(), 2);
        assert_eq!(dns[0].to_string(), "8.8.8.8");
        assert_eq!(dns[1].to_string(), "8.8.4.4");
    }

    #[test]
    fn test_add_network_options_no_dns() {
        use chrono::Utc;

        let network = DhcpNetwork {
            id: 1,
            name: "test-network".to_string(),
            subnet: "10.0.0.0/24".to_string(),
            gateway: "10.0.0.1".to_string(),
            dns_servers: vec![],
            lease_duration: 3600,
            relay_agent_address: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        let mut msg = Message::default();
        add_network_options(&mut msg, &network).unwrap();

        // Verify DNS servers option is not present
        let dns = msg
            .opts()
            .iter()
            .find(|(_, opt)| matches!(opt, v4::DhcpOption::DomainNameServer(_)));
        assert!(dns.is_none());
    }

    #[test]
    fn test_build_nak() {
        use dhcproto::v4::Flags;

        let mut req = Message::default();
        req.set_opcode(Opcode::BootRequest);
        req.set_xid(0x12345678);
        req.set_chaddr(&[0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff]);
        req.set_flags(Flags::default());

        let server_id: Ipv4Addr = "10.0.0.1".parse().unwrap();
        let nak = build_nak(&req, server_id);

        // Verify base fields
        assert_eq!(nak.opcode(), Opcode::BootReply);
        assert_eq!(nak.xid(), 0x12345678);
        assert_eq!(nak.chaddr(), &[0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff]);

        // Verify message type
        let msg_type = nak
            .opts()
            .iter()
            .find_map(|(_, opt)| {
                if let v4::DhcpOption::MessageType(mt) = opt {
                    Some(*mt)
                } else {
                    None
                }
            })
            .expect("MessageType should be present");
        assert_eq!(msg_type, MessageType::Nak);

        // Verify server identifier
        let server_identifier = nak
            .opts()
            .iter()
            .find_map(|(_, opt)| {
                if let v4::DhcpOption::ServerIdentifier(ip) = opt {
                    Some(*ip)
                } else {
                    None
                }
            })
            .expect("ServerIdentifier should be present");
        assert_eq!(server_identifier, server_id);
    }
}
