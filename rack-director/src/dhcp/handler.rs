use anyhow::Result;
use dhcproto::{
    Decodable, Encodable,
    decoder::Decoder,
    encoder::Encoder,
    v4::{self, Message, MessageType, Opcode},
};
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use tokio::net::UdpSocket;

use crate::director::Director;

use super::allocator::IpAllocator;
use super::boot_config::{BootConfigProvider, BootMode};
use super::store::{DhcpStore, LeaseState, format_mac};

#[derive(Clone)]
pub struct DhcpHandler {
    store: DhcpStore,
    director: Director,
    allocator: IpAllocator,
    boot_config: BootConfigProvider,
}

impl DhcpHandler {
    pub fn new(
        store: DhcpStore,
        director: Director,
        allocator: IpAllocator,
        boot_config: BootConfigProvider,
    ) -> Self {
        Self {
            store,
            director,
            allocator,
            boot_config,
        }
    }

    pub async fn handle_packet(
        &self,
        data: &[u8],
        _peer_addr: SocketAddr,
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

        let response = match msg.opts().msg_type() {
            Some(MessageType::Discover) => self.handle_discover(msg).await?,
            Some(MessageType::Request) => self.handle_request(msg).await?,
            Some(MessageType::Release) => {
                self.handle_release(msg).await?;
                None
            }
            Some(MessageType::Decline) => {
                self.handle_decline(msg).await?;
                None
            }
            _ => {
                log::debug!("Ignoring unsupported DHCP message type");
                return Ok(());
            }
        };

        if let Some(resp) = response {
            let mut buf = Vec::new();
            resp.encode(&mut Encoder::new(&mut buf))?;

            // Broadcast response (DHCP protocol requirement)
            let broadcast_addr = SocketAddr::from(([255, 255, 255, 255], 68));
            socket.send_to(&buf, broadcast_addr).await?;
        }

        Ok(())
    }

    async fn handle_discover(&self, msg: Message) -> Result<Option<Message>> {
        let mac = msg.chaddr();
        let mac_str = format_mac(mac);

        log::info!("DHCP DISCOVER from MAC {}", mac_str);

        // Check if device exists in devices table by MAC
        let device_uuid = self.director.find_device_by_mac(&mac_str).await?;

        // Allocate or retrieve existing IP
        let ip = if let Some(uuid) = &device_uuid {
            log::debug!("Device UUID {} found for MAC {}", uuid, mac_str);
            self.allocator.allocate_for_device(&mac_str, uuid).await?
        } else {
            log::debug!(
                "No device UUID found for MAC {}, allocating from pool",
                mac_str
            );
            self.allocator.allocate_for_mac(&mac_str).await?
        };

        // Create lease in 'offered' state
        self.store
            .create_or_update_lease(
                &mac_str,
                &ip,
                device_uuid.as_deref(),
                LeaseState::Offered,
                self.allocator.config().lease_duration,
            )
            .await?;

        // Build DHCP Offer
        let offer = self.build_offer(&msg, ip)?;
        log::info!("DHCP OFFER {} to MAC {}", ip, mac_str);

        Ok(Some(offer))
    }

    async fn handle_request(&self, msg: Message) -> Result<Option<Message>> {
        let mac = msg.chaddr();
        let mac_str = format_mac(mac);

        log::info!("DHCP REQUEST from MAC {}", mac_str);

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
                return Ok(Some(self.build_nak(&msg)?));
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
                return Ok(Some(self.build_nak(&msg)?));
            }

            // Update lease to 'active'
            self.store.activate_lease(&mac_str).await?;

            // Build DHCP Ack with boot options
            let ack = self.build_ack(&msg, lease_ip)?;
            log::info!("DHCP ACK {} to MAC {}", requested_ip, mac_str);

            Ok(Some(ack))
        } else {
            log::warn!("No lease found for MAC {}", mac_str);
            Ok(Some(self.build_nak(&msg)?))
        }
    }

    async fn handle_release(&self, msg: Message) -> Result<()> {
        let mac = msg.chaddr();
        let mac_str = format_mac(mac);

        log::info!("DHCP RELEASE from MAC {}", mac_str);

        self.store.release_lease(&mac_str).await?;

        Ok(())
    }

    async fn handle_decline(&self, msg: Message) -> Result<()> {
        let mac = msg.chaddr();
        let mac_str = format_mac(mac);

        log::warn!("DHCP DECLINE from MAC {}", mac_str);

        // Mark lease as released to prevent reuse
        self.store.release_lease(&mac_str).await?;

        Ok(())
    }

    fn build_offer(&self, req: &Message, ip: Ipv4Addr) -> Result<Message> {
        let mut msg = Message::default();
        msg.set_opcode(Opcode::BootReply);
        msg.set_xid(req.xid());
        msg.set_yiaddr(ip);
        msg.set_chaddr(req.chaddr());
        msg.set_flags(req.flags());

        let config = self.allocator.config();

        // Standard DHCP options
        msg.opts_mut()
            .insert(v4::DhcpOption::MessageType(MessageType::Offer));
        msg.opts_mut().insert(v4::DhcpOption::ServerIdentifier(
            config.tftp_server.parse()?,
        ));
        msg.opts_mut()
            .insert(v4::DhcpOption::AddressLeaseTime(config.lease_duration));

        // Network configuration
        let subnet_mask = self.calculate_subnet_mask(&config.subnet)?;
        msg.opts_mut()
            .insert(v4::DhcpOption::SubnetMask(subnet_mask));
        msg.opts_mut()
            .insert(v4::DhcpOption::Router(vec![config.gateway.parse()?]));

        let dns_servers: Vec<Ipv4Addr> = config
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

    fn build_ack(&self, req: &Message, ip: Ipv4Addr) -> Result<Message> {
        let mut msg = self.build_offer(req, ip)?;
        msg.opts_mut()
            .insert(v4::DhcpOption::MessageType(MessageType::Ack));

        // Determine boot mode from DHCP options
        let boot_mode = self.detect_boot_mode(req);
        let boot_opts = self.boot_config.get_boot_options(boot_mode)?;

        log::debug!(
            "Boot mode: {:?}, next_server: {}, filename: {}",
            boot_mode,
            boot_opts.next_server,
            boot_opts.filename
        );

        // Option 66 (TFTP Server Name)
        msg.opts_mut().insert(v4::DhcpOption::TFTPServerName(
            boot_opts.next_server.clone().into_bytes(),
        ));

        // Option 67 (Bootfile Name)
        msg.opts_mut().insert(v4::DhcpOption::BootfileName(
            boot_opts.filename.into_bytes(),
        ));

        // siaddr field (next server IP)
        let next_server_ip: Ipv4Addr = boot_opts.next_server.parse()?;
        msg.set_siaddr(next_server_ip);

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

        let config = self.allocator.config();
        msg.opts_mut().insert(v4::DhcpOption::ServerIdentifier(
            config.tftp_server.parse()?,
        ));

        Ok(msg)
    }

    fn detect_boot_mode(&self, msg: &Message) -> BootMode {
        // Check Option 93 (Client System Architecture)
        for (_code, opt) in msg.opts().iter() {
            if let v4::DhcpOption::ClientSystemArchitecture(arch) = opt {
                // Format the architecture as a string to match against
                let arch_str = format!("{:?}", arch);
                return if arch_str.contains("EFI") {
                    if arch_str.contains("Arm") || arch_str.contains("AArch64") {
                        BootMode::UefiArm64
                    } else {
                        BootMode::UefiBoot
                    }
                } else {
                    BootMode::BiosLegacy
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

    fn create_test_handler() -> DhcpHandler {
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
        let director = Director::new(db);

        // Use tokio runtime for async operation
        let rt = tokio::runtime::Runtime::new().unwrap();
        let config = rt.block_on(store.load_config()).unwrap();

        let allocator = IpAllocator::new(store.clone(), director.clone(), config.clone());
        let boot_config =
            BootConfigProvider::new(config.tftp_server.clone(), config.http_server.clone());

        DhcpHandler::new(store, director, allocator, boot_config)
    }
}
