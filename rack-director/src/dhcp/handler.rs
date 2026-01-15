use anyhow::Result;
use dhcproto::{
    Decodable, Encodable,
    decoder::Decoder,
    encoder::Encoder,
    v4::{self, Architecture, Message, MessageType, Opcode},
};
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use tokio::net::UdpSocket;

use crate::director::Director;

use super::allocator::IpAllocator;
use super::boot_config::{BootConfigProvider, BootMode};
use super::store::{DhcpNetwork, DhcpStore, LeaseState, format_mac};

#[derive(Clone)]
pub struct DhcpHandler {
    store: DhcpStore,
    director: Director,
    allocator: IpAllocator,
    boot_config: BootConfigProvider,
    server_identifier: Ipv4Addr,
}

impl DhcpHandler {
    pub fn new(
        store: DhcpStore,
        director: Director,
        allocator: IpAllocator,
        boot_config: BootConfigProvider,
        server_identifier: Ipv4Addr,
    ) -> Self {
        Self {
            store,
            director,
            allocator,
            boot_config,
            server_identifier,
        }
    }

    pub async fn handle_packet(
        &self,
        data: &[u8],
        peer_addr: SocketAddr,
        socket: Arc<UdpSocket>,
    ) -> Result<()> {
        // Decode DHCP message using dhcproto
        let msg = match Message::decode(&mut Decoder::new(data)) {
            Ok(msg) => msg,
            Err(e) => {
                log::warn!("Failed to decode DHCP message: {}", e);
                return Ok(());
            }
        };

        // Extract relay agent address (giaddr)
        let relay_agent = if msg.giaddr() != Ipv4Addr::UNSPECIFIED {
            Some(msg.giaddr())
        } else {
            None
        };

        // Match to network based on relay agent
        let network = match self.store.get_network_by_relay(relay_agent).await? {
            Some(network) => network,
            None => {
                log::warn!("No network found for relay agent {:?}", relay_agent);
                return Ok(());
            }
        };

        log::debug!(
            "Using network '{}' (id={}) for relay {:?}",
            network.name,
            network.id,
            relay_agent
        );

        let response = match msg.opts().msg_type() {
            Some(MessageType::Discover) => self.handle_discover(&msg, &network).await?,
            Some(MessageType::Request) => self.handle_request(&msg, &network).await?,
            Some(MessageType::Release) => {
                self.handle_release(&msg).await?;
                None
            }
            Some(MessageType::Decline) => {
                self.handle_decline(&msg).await?;
                None
            }
            _ => {
                log::debug!("Ignoring unsupported DHCP message type");
                return Ok(());
            }
        };

        if let Some(resp) = response {
            self.send_response(resp, &msg, peer_addr, socket).await?;
        }

        Ok(())
    }

    /// Send DHCP response following RFC 3046 relay agent rules
    async fn send_response(
        &self,
        resp: Message,
        req: &Message,
        peer_addr: SocketAddr,
        socket: Arc<UdpSocket>,
    ) -> Result<()> {
        let mut buf = Vec::new();
        resp.encode(&mut Encoder::new(&mut buf))?;

        // RFC 3046: If giaddr is set, send to relay agent on port 67
        // Otherwise, send to peer (broadcast or unicast)
        let dest = if req.giaddr() != Ipv4Addr::UNSPECIFIED {
            SocketAddr::new(req.giaddr().into(), 67)
        } else {
            // For localhost testing, we send unicast to the peer address
            // In production, this would be broadcast to 255.255.255.255:68
            peer_addr
        };

        socket.send_to(&buf, dest).await?;
        log::debug!("Sent DHCP response to {}", dest);

        Ok(())
    }

    async fn handle_discover(
        &self,
        msg: &Message,
        network: &DhcpNetwork,
    ) -> Result<Option<Message>> {
        let mac = msg.chaddr();
        let mac_str = format_mac(mac);

        log::info!(
            "DHCP DISCOVER from MAC {} on network '{}'",
            mac_str,
            network.name
        );

        // Check if device exists in devices table by MAC
        let device_uuid = self.director.find_device_by_mac(&mac_str).await?;

        // Check if this interface is disabled (e.g., due to duplicate MAC)
        if let Some(uuid) = &device_uuid {
            let interfaces = self.director.get_network_interfaces(uuid).await?;

            if let Some(iface) = interfaces.iter().find(|i| i.mac_address == mac_str) {
                if iface.disabled {
                    log::warn!(
                        "Skipping DHCP DISCOVER for disabled interface {} on device {}. Reason: {}",
                        mac_str, uuid,
                        iface.warning_label.as_deref().unwrap_or("unknown")
                    );
                    return Ok(None);
                }
            }
        }

        // Allocate or retrieve existing IP in this network
        let ip = if let Some(uuid) = &device_uuid {
            log::debug!("Device UUID {} found for MAC {}", uuid, mac_str);
            self.allocator
                .allocate_for_device_in_network(&mac_str, uuid, network.id)
                .await?
        } else {
            log::debug!(
                "No device UUID found for MAC {}, allocating from pool",
                mac_str
            );
            self.allocator
                .allocate_for_mac_in_network(&mac_str, network.id)
                .await?
        };

        // Create lease in 'offered' state
        self.store
            .create_or_update_lease_with_network(
                &mac_str,
                &ip,
                device_uuid.as_deref(),
                LeaseState::Offered,
                network.lease_duration,
                network.id,
            )
            .await?;

        // Build DHCP Offer
        let offer = self.build_offer(msg, ip, network)?;
        log::info!(
            "DHCP OFFER {} to MAC {} on network '{}'",
            ip,
            mac_str,
            network.name
        );

        Ok(Some(offer))
    }

    async fn handle_request(
        &self,
        msg: &Message,
        network: &DhcpNetwork,
    ) -> Result<Option<Message>> {
        let mac = msg.chaddr();
        let mac_str = format_mac(mac);

        log::info!(
            "DHCP REQUEST from MAC {} on network '{}'",
            mac_str,
            network.name
        );

        // Look up device UUID early for authorization checks
        let device_uuid = self.director.find_device_by_mac(&mac_str).await?;

        // Check if device is in pending_devices table
        let is_pending_device = self.director.find_pending_device_by_mac(&mac_str).await?.is_some();

        // Check if this interface is disabled (e.g., due to duplicate MAC)
        if let Some(uuid) = &device_uuid {
            let interfaces = self.director.get_network_interfaces(uuid).await?;

            if let Some(iface) = interfaces.iter().find(|i| i.mac_address == mac_str) {
                if iface.disabled {
                    log::warn!(
                        "Skipping DHCP REQUEST for disabled interface {} on device {}. Reason: {}",
                        mac_str, uuid,
                        iface.warning_label.as_deref().unwrap_or("unknown")
                    );
                    return Ok(None);
                }
            }
        }

        // Extract requested IP address
        let requested_ip = if let Some((_code, v4::DhcpOption::RequestedIpAddress(ip))) = msg
            .opts()
            .iter()
            .find(|(_, opt)| matches!(opt, v4::DhcpOption::RequestedIpAddress(_)))
        {
            *ip
        } else {
            // No requested IP, check ciaddr (client IP address)
            if msg.ciaddr() != Ipv4Addr::UNSPECIFIED {
                msg.ciaddr()
            } else {
                log::warn!("DHCP REQUEST without requested IP or ciaddr");
                return Ok(Some(self.build_nak(msg)?));
            }
        };

        log::debug!("Requested IP: {}", requested_ip);

        // Validate request matches our offer
        let lease = self.store.get_lease_by_mac(&mac_str).await?;
        if let Some(lease) = lease {
            let lease_ip: Ipv4Addr = lease.ip_address.parse()?;
            if lease_ip != requested_ip {
                log::warn!(
                    "DHCP REQUEST IP mismatch: requested {}, expected {}",
                    requested_ip,
                    lease_ip
                );
                return Ok(Some(self.build_nak(msg)?));
            }

            // Update lease to 'active'
            self.store.activate_lease(&mac_str).await?;
            if let Some(uuid) = &device_uuid {
                self.director
                    .set_device_ip_address(uuid, &lease_ip.to_string())
                    .await?;
            }

            // Build DHCP Ack with boot options (passing device_uuid for authorization)
            let ack = self.build_ack(msg, lease_ip, network, device_uuid.as_deref(), is_pending_device)?;
            log::info!(
                "DHCP ACK {} to MAC {} on network '{}'",
                requested_ip,
                mac_str,
                network.name
            );

            Ok(Some(ack))
        } else {
            log::warn!("No lease found for MAC {}", mac_str);
            Ok(Some(self.build_nak(msg)?))
        }
    }

    async fn handle_release(&self, msg: &Message) -> Result<()> {
        let mac = msg.chaddr();
        let mac_str = format_mac(mac);

        log::info!("DHCP RELEASE from MAC {}", mac_str);

        self.store.release_lease(&mac_str).await?;

        Ok(())
    }

    async fn handle_decline(&self, msg: &Message) -> Result<()> {
        let mac = msg.chaddr();
        let mac_str = format_mac(mac);

        log::warn!("DHCP DECLINE from MAC {}", mac_str);

        // Mark lease as released to prevent reuse
        self.store.release_lease(&mac_str).await?;

        Ok(())
    }

    fn build_offer(&self, req: &Message, ip: Ipv4Addr, network: &DhcpNetwork) -> Result<Message> {
        let mut msg = Message::default();
        msg.set_opcode(Opcode::BootReply);
        msg.set_xid(req.xid());
        msg.set_yiaddr(ip);
        msg.set_chaddr(req.chaddr());
        msg.set_flags(req.flags());

        // Standard DHCP options
        msg.opts_mut()
            .insert(v4::DhcpOption::MessageType(MessageType::Offer));
        msg.opts_mut()
            .insert(v4::DhcpOption::ServerIdentifier(self.server_identifier));
        msg.opts_mut()
            .insert(v4::DhcpOption::AddressLeaseTime(network.lease_duration));

        // Network configuration
        let subnet_mask = self.calculate_subnet_mask(&network.subnet)?;
        msg.opts_mut()
            .insert(v4::DhcpOption::SubnetMask(subnet_mask));
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

        Ok(msg)
    }

    fn build_ack(
        &self,
        req: &Message,
        ip: Ipv4Addr,
        network: &DhcpNetwork,
        device_uuid: Option<&str>,
        is_pending_device: bool,
    ) -> Result<Message> {
        let mut msg = self.build_offer(req, ip, network)?;
        msg.opts_mut()
            .insert(v4::DhcpOption::MessageType(MessageType::Ack));

        // Check if this is iPXE making a second DHCP request
        let is_ipxe = self.is_ipxe(req);

        if is_ipxe {
            // iPXE second-stage boot: Return HTTP URL for boot script (if allowed)
            if let Some(boot_opts) = self
                .boot_config
                .get_ipxe_boot_script_if_allowed(device_uuid, is_pending_device)?
            {
                log::debug!(
                    "iPXE detected, returning HTTP boot script: {}",
                    boot_opts.filename
                );

                // Option 67 (Bootfile Name) - HTTP URL for iPXE script
                msg.opts_mut().insert(v4::DhcpOption::BootfileName(
                    boot_opts.filename.into_bytes(),
                ));
            } else {
                log::info!("Skipping boot script for unknown device (autodiscover disabled)");
            }
        } else {
            // First-stage boot: Determine boot mode and return bootloader via TFTP (if allowed)
            let boot_mode = self.detect_boot_mode(req);
            if let Some(boot_opts) = self
                .boot_config
                .get_boot_options_if_allowed(boot_mode, device_uuid, is_pending_device)?
            {
                log::debug!(
                    "Boot mode: {:?}, next_server: {:?}, filename: {}",
                    boot_mode,
                    boot_opts.next_server,
                    boot_opts.filename
                );

                // Option 66 (TFTP Server Name)
                if let Some(next_server) = &boot_opts.next_server {
                    msg.opts_mut().insert(v4::DhcpOption::TFTPServerName(
                        next_server.clone().into_bytes(),
                    ));
                }

                // Option 67 (Bootfile Name)
                msg.opts_mut().insert(v4::DhcpOption::BootfileName(
                    boot_opts.filename.into_bytes(),
                ));

                // siaddr field (next server IP)
                if let Some(next_server) = &boot_opts.next_server
                    && let Ok(next_ip) = next_server.parse::<Ipv4Addr>()
                {
                    msg.set_siaddr(next_ip);
                }
            } else {
                log::info!("Skipping boot options for unknown device (autodiscover disabled)");
            }
        }

        Ok(msg)
    }

    fn build_nak(&self, req: &Message) -> Result<Message> {
        let mut msg = Message::default();
        msg.set_opcode(Opcode::BootReply);
        msg.set_xid(req.xid());
        msg.set_chaddr(req.chaddr());
        msg.set_flags(req.flags());

        msg.opts_mut()
            .insert(v4::DhcpOption::MessageType(MessageType::Nak));

        msg.opts_mut()
            .insert(v4::DhcpOption::ServerIdentifier(self.server_identifier));

        Ok(msg)
    }

    fn is_ipxe(&self, msg: &Message) -> bool {
        // Check Option 77 (User-Class) for "iPXE" identifier
        for (_code, opt) in msg.opts().iter() {
            if let v4::DhcpOption::UserClass(data) = opt {
                // The UserClass option contains a Vec<u8>
                // iPXE sends "iPXE" as the user class identifier
                if data == b"iPXE" {
                    return true;
                }
            }
        }
        false
    }

    fn detect_boot_mode(&self, msg: &Message) -> BootMode {
        // Check Option 93 (Client System Architecture)
        for (_code, opt) in msg.opts().iter() {
            if let v4::DhcpOption::ClientSystemArchitecture(arch) = opt {
                return match arch {
                    Architecture::Intelx86PC => BootMode::BiosLegacy, // Intel x86PC
                    Architecture::BC | Architecture::X86_64 => BootMode::UefiBoot, // EFI IA32, EFI BC (x86-64), EFI Xscale
                    Architecture::Unknown(11) => BootMode::UefiArm64, // EFI ARM 64-bit (AArch64)
                    _ => {
                        // Unknown architecture, assume BIOS for safety
                        log::warn!("Unknown client architecture: {:?}", arch);
                        BootMode::BiosLegacy
                    }
                };
            }
        }

        // No architecture option: assume BIOS
        BootMode::BiosLegacy
    }

    fn calculate_subnet_mask(&self, subnet: &str) -> Result<Ipv4Addr> {
        // Parse CIDR notation (e.g., "10.0.0.0/24")
        let parts: Vec<&str> = subnet.split('/').collect();
        if parts.len() != 2 {
            return Err(anyhow::anyhow!("Invalid subnet format: {}", subnet));
        }

        let prefix_len: u8 = parts[1].parse()?;
        if prefix_len > 32 {
            return Err(anyhow::anyhow!("Invalid prefix length: {}", prefix_len));
        }

        // Calculate netmask from prefix length
        let mask = if prefix_len == 0 {
            0u32
        } else {
            !0u32 << (32 - prefix_len)
        };

        Ok(Ipv4Addr::from(mask))
    }
}

#[cfg(test)]
mod tests {
    use crate::storage::MemoryImageStore;

    use super::*;

    #[test]
    fn test_calculate_subnet_mask() {
        let handler = create_test_handler();

        assert_eq!(
            handler
                .calculate_subnet_mask("10.0.0.0/24")
                .unwrap()
                .to_string(),
            "255.255.255.0"
        );
        assert_eq!(
            handler
                .calculate_subnet_mask("10.0.0.0/16")
                .unwrap()
                .to_string(),
            "255.255.0.0"
        );
        assert_eq!(
            handler
                .calculate_subnet_mask("10.0.0.0/8")
                .unwrap()
                .to_string(),
            "255.0.0.0"
        );
    }

    #[test]
    fn test_server_identifier_in_offer() {
        let (handler, store) = create_test_handler_with_store();

        // Create a minimal DISCOVER message
        let mut discover = Message::default();
        discover.set_opcode(Opcode::BootRequest);
        discover.set_xid(0x12345678);
        discover.set_chaddr(&[0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff]);
        discover
            .opts_mut()
            .insert(v4::DhcpOption::MessageType(MessageType::Discover));

        // Get default network
        let rt = tokio::runtime::Runtime::new().unwrap();
        let network = rt.block_on(store.get_network(1)).unwrap();

        // Build an OFFER response
        let offer = handler
            .build_offer(&discover, "10.0.0.100".parse().unwrap(), &network)
            .unwrap();

        // Verify the server identifier matches the handler's configured value
        let server_id = offer
            .opts()
            .iter()
            .find_map(|(_, opt)| {
                if let v4::DhcpOption::ServerIdentifier(ip) = opt {
                    Some(*ip)
                } else {
                    None
                }
            })
            .expect("Server Identifier should be present in OFFER");

        assert_eq!(
            server_id, handler.server_identifier,
            "Server Identifier in OFFER should match handler's configured value"
        );
    }

    #[test]
    fn test_server_identifier_in_nak() {
        let handler = create_test_handler();

        // Create a minimal REQUEST message
        let mut request = Message::default();
        request.set_opcode(Opcode::BootRequest);
        request.set_xid(0x12345678);
        request.set_chaddr(&[0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff]);
        request
            .opts_mut()
            .insert(v4::DhcpOption::MessageType(MessageType::Request));

        // Build a NAK response
        let nak = handler.build_nak(&request).unwrap();

        // Verify the server identifier matches the handler's configured value
        let server_id = nak
            .opts()
            .iter()
            .find_map(|(_, opt)| {
                if let v4::DhcpOption::ServerIdentifier(ip) = opt {
                    Some(*ip)
                } else {
                    None
                }
            })
            .expect("Server Identifier should be present in NAK");

        assert_eq!(
            server_id, handler.server_identifier,
            "Server Identifier in NAK should match handler's configured value"
        );
    }

    #[test]
    fn test_custom_server_identifier() {
        use crate::database;
        use crate::director::Director;
        use std::sync::Arc;
        use tempfile::tempdir;
        use tokio::sync::Mutex;

        let temp_dir = tempdir().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let conn = database::open(db_path).unwrap();
        let db = Arc::new(Mutex::new(conn));
        let store = DhcpStore::new(db.clone());
        let director = Director::new(
            db.clone(),
            Arc::new(MemoryImageStore::new()),
            "http://localhost:8080",
        );

        let rt = tokio::runtime::Runtime::new().unwrap();

        let allocator = IpAllocator::new(store.clone());
        let boot_config =
            BootConfigProvider::new("10.0.0.1".to_string(), "10.0.0.1".to_string(), false);

        // Use a custom server identifier different from gateway
        let custom_server_id: Ipv4Addr = "192.168.1.50".parse().unwrap();
        let handler = DhcpHandler::new(
            store.clone(),
            director,
            allocator,
            boot_config,
            custom_server_id,
        );

        // Verify the handler stores the custom value
        assert_eq!(
            handler.server_identifier, custom_server_id,
            "Handler should store custom server identifier"
        );

        // Get default network
        let network = rt.block_on(store.get_network(1)).unwrap();

        // Build an OFFER and verify it uses the custom identifier
        let mut discover = Message::default();
        discover.set_opcode(Opcode::BootRequest);
        discover.set_xid(0x12345678);
        discover.set_chaddr(&[0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff]);
        discover
            .opts_mut()
            .insert(v4::DhcpOption::MessageType(MessageType::Discover));

        let offer = handler
            .build_offer(&discover, "10.0.0.100".parse().unwrap(), &network)
            .unwrap();

        let server_id = offer
            .opts()
            .iter()
            .find_map(|(_, opt)| {
                if let v4::DhcpOption::ServerIdentifier(ip) = opt {
                    Some(*ip)
                } else {
                    None
                }
            })
            .expect("Server Identifier should be present in OFFER");

        assert_eq!(
            server_id, custom_server_id,
            "OFFER should use custom server identifier, not gateway"
        );
    }

    fn create_test_handler_with_store() -> (DhcpHandler, DhcpStore) {
        use crate::database;
        use crate::director::Director;
        use std::sync::Arc;
        use tempfile::tempdir;
        use tokio::sync::Mutex;

        let temp_dir = tempdir().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let conn = database::open(db_path).unwrap();
        let db = Arc::new(Mutex::new(conn));
        let store = DhcpStore::new(db.clone());
        let director = Director::new(
            db.clone(),
            Arc::new(MemoryImageStore::new()),
            "http://localhost:8080",
        );

        let allocator = IpAllocator::new(store.clone());
        let boot_config =
            BootConfigProvider::new("10.0.0.1".to_string(), "10.0.0.1".to_string(), false);
        let server_identifier = "10.0.0.1".parse().unwrap();

        let handler = DhcpHandler::new(
            store.clone(),
            director,
            allocator,
            boot_config,
            server_identifier,
        );
        (handler, store)
    }

    fn create_test_handler() -> DhcpHandler {
        let (handler, _store) = create_test_handler_with_store();
        handler
    }
}
