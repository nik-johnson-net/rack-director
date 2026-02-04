use anyhow::Result;
use dhcproto::{
    Decodable, Encodable,
    decoder::Decoder,
    encoder::Encoder,
    v4::{self, Architecture, Message, MessageType},
};
use log::{debug, info, trace, warn};
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use tokio::net::UdpSocket;
use uuid::Uuid;

use super::allocator::IpAllocator;
use super::boot_config::BootConfigProvider;
use super::device_resolution::{DeviceContext, DeviceResolver};
use super::message_builder;
use super::request::RequestContext;
use super::store::{DhcpNetwork, DhcpStore, LeaseState, format_mac};

/// Determines the appropriate vendor class identifier (Option 60) based on client architecture.
///
/// # DHCP Option 60 - Vendor Class Identifier
///
/// This option is used by PXE clients to identify themselves and by servers to provide
/// architecture-specific boot information. Different client types expect different identifiers:
///
/// - **HTTP Boot clients** (arch 14/15/16): Expect "HTTPClient" to indicate HTTP boot support
/// - **Traditional PXE clients** (all other architectures): Expect "PXEServer" identifier
///
/// # Arguments
/// * `client_arch` - The client architecture from DHCP Option 93, if present
///
/// # Returns
/// The vendor class identifier as a byte slice:
/// - `b"HTTPClient"` for HTTP boot architectures (14, 15, 16)
/// - `b"PXEServer"` for all other cases (traditional PXE boot)
///
/// # References
/// - RFC 4578 - DHCP PXE Options
/// - UEFI Specification 2.10 - Section 24.4.2 (HTTP Boot)
fn determine_vendor_class_identifier(client_arch: Option<Architecture>) -> &'static [u8] {
    match client_arch {
        // HTTP Boot architectures
        // - 14: x86-64 HTTP
        // - 15: x86 HTTP (IA32)
        // - 16: EBC (EFI Byte Code) HTTP
        Some(Architecture::Unknown(14) | Architecture::Unknown(15) | Architecture::Unknown(16)) => {
            b"HTTPClient"
        }
        // All other architectures (traditional PXE boot)
        _ => b"PXEServer",
    }
}

/// Determines whether boot options should be provided for a device.
///
/// # Decision Logic
/// - If network autodiscover is enabled: Always provide boot options (permissive mode)
/// - If network autodiscover is disabled: Only provide boot options for known devices (strict mode)
/// - Pending devices (in pending_devices table) are also allowed to boot
///
/// # Arguments
/// * `network_autodiscover` - Whether autodiscovery is enabled for this network
/// * `device_uuid` - The device UUID if the device is known (exists in devices table)
/// * `is_pending_device` - Whether the device exists in the pending_devices table
///
/// # Returns
/// `true` if boot options should be provided, `false` otherwise
fn should_provide_boot_options(
    network_autodiscover: bool,
    device_uuid: Option<&Uuid>,
    is_pending_device: bool,
) -> bool {
    network_autodiscover || device_uuid.is_some() || is_pending_device
}

#[derive(Clone)]
pub struct DhcpHandler {
    store: DhcpStore,
    device_resolver: Arc<dyn DeviceResolver>,
    allocator: IpAllocator,
    boot_config: BootConfigProvider,
    server_identifier: Ipv4Addr,
}

impl DhcpHandler {
    pub fn new(
        store: DhcpStore,
        device_resolver: Arc<dyn DeviceResolver>,
        allocator: IpAllocator,
        boot_config: BootConfigProvider,
        server_identifier: Ipv4Addr,
    ) -> Self {
        Self {
            store,
            device_resolver,
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

        trace!("DHCP: Received packet {:?}", msg);

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

        debug!(
            "Using network '{}' (id={}) for relay {:?}",
            network.name, network.id, relay_agent
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
            trace!("DHCP: Sending response {:?}", resp);
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
            if peer_addr.ip().is_unspecified() {
                SocketAddr::new(Ipv4Addr::BROADCAST.into(), 68)
            } else {
                peer_addr
            }
        };

        debug!("Sending DHCP response to {}", dest);
        socket.send_to(&buf, dest).await?;

        Ok(())
    }

    async fn handle_discover(
        &self,
        msg: &Message,
        network: &DhcpNetwork,
    ) -> Result<Option<Message>> {
        let req_ctx = RequestContext::from_message(msg);

        info!(
            "DHCP DISCOVER from MAC {} on network '{}'",
            req_ctx.mac, network.name
        );

        let dev_ctx = self.device_resolver.resolve(&req_ctx.mac).await?;

        if dev_ctx.is_disabled {
            warn!(
                "Skipping DHCP DISCOVER for disabled interface {} on device {}. Reason: {}",
                req_ctx.mac,
                dev_ctx
                    .device_uuid
                    .as_ref()
                    .map(|u| u.to_string())
                    .unwrap_or_default(),
                dev_ctx.disable_reason.as_deref().unwrap_or("unknown")
            );
            return Ok(None);
        }

        // Allocate or retrieve existing IP in this network
        let ip = if let Some(uuid) = &dev_ctx.device_uuid {
            debug!("Device UUID {} found for MAC {}", uuid, req_ctx.mac);
            self.allocator
                .allocate_for_device_in_network(&req_ctx.mac, uuid, network.id)
                .await?
        } else {
            debug!(
                "No device UUID found for MAC {}, allocating from pool",
                req_ctx.mac
            );
            self.allocator
                .allocate_for_mac_in_network(&req_ctx.mac, network.id)
                .await?
        };

        // Create lease in 'offered' state
        self.store
            .create_or_update_lease_with_network(
                &req_ctx.mac,
                &ip,
                dev_ctx.device_uuid.as_ref(),
                LeaseState::Offered,
                network.lease_duration,
                network.id,
            )
            .await?;

        let offer = self
            .build_offer(msg, ip, network, &req_ctx, &dev_ctx)
            .await?;
        info!(
            "DHCP OFFER {} to MAC {} on network '{}'",
            ip, req_ctx.mac, network.name
        );

        Ok(Some(offer))
    }

    async fn handle_request(
        &self,
        msg: &Message,
        network: &DhcpNetwork,
    ) -> Result<Option<Message>> {
        let req_ctx = RequestContext::from_message(msg);

        info!(
            "DHCP REQUEST from MAC {} on network '{}'",
            req_ctx.mac, network.name
        );

        let dev_ctx = self.device_resolver.resolve(&req_ctx.mac).await?;

        if dev_ctx.is_disabled {
            warn!(
                "Skipping DHCP REQUEST for disabled interface {} on device {}. Reason: {}",
                req_ctx.mac,
                dev_ctx
                    .device_uuid
                    .as_ref()
                    .map(|u| u.to_string())
                    .unwrap_or_default(),
                dev_ctx.disable_reason.as_deref().unwrap_or("unknown")
            );
            return Ok(None);
        }

        // Extract requested IP address
        let requested_ip = if let Some(ip) = req_ctx.requested_ip {
            ip
        } else if req_ctx.ciaddr != Ipv4Addr::UNSPECIFIED {
            req_ctx.ciaddr
        } else {
            warn!("DHCP REQUEST without requested IP or ciaddr");
            return Ok(Some(self.build_nak(msg)?));
        };

        debug!("Requested IP: {}", requested_ip);

        // Validate request matches our offer
        let lease = self.store.get_lease_by_mac(&req_ctx.mac).await?;
        if let Some(lease) = lease {
            let lease_ip: Ipv4Addr = lease.ip_address.parse()?;
            if lease_ip != requested_ip {
                warn!(
                    "DHCP REQUEST IP mismatch: requested {}, expected {}",
                    requested_ip, lease_ip
                );
                return Ok(Some(self.build_nak(msg)?));
            }

            // Update lease to 'active'
            self.store.activate_lease(&req_ctx.mac).await?;
            if let Some(uuid) = &dev_ctx.device_uuid {
                self.device_resolver
                    .on_lease_activated(uuid, &lease_ip.to_string(), &req_ctx.mac)
                    .await?;
            }

            let ack = self
                .build_ack(msg, lease_ip, network, &req_ctx, &dev_ctx)
                .await?;
            info!(
                "DHCP ACK {} to MAC {} on network '{}'",
                requested_ip, req_ctx.mac, network.name
            );

            Ok(Some(ack))
        } else {
            warn!("No lease found for MAC {}", req_ctx.mac);
            Ok(Some(self.build_nak(msg)?))
        }
    }

    async fn handle_release(&self, msg: &Message) -> Result<()> {
        let mac = msg.chaddr();
        let mac_str = format_mac(mac);

        info!("DHCP RELEASE from MAC {}", mac_str);

        self.store.release_lease(&mac_str).await?;

        Ok(())
    }

    async fn handle_decline(&self, msg: &Message) -> Result<()> {
        let mac = msg.chaddr();
        let mac_str = format_mac(mac);

        warn!("DHCP DECLINE from MAC {}", mac_str);

        // Mark lease as released to prevent reuse
        self.store.release_lease(&mac_str).await?;

        Ok(())
    }

    async fn build_offer(
        &self,
        req: &Message,
        ip: Ipv4Addr,
        network: &DhcpNetwork,
        req_ctx: &RequestContext,
        dev_ctx: &DeviceContext,
    ) -> Result<Message> {
        let mut msg = message_builder::create_base_reply(req, &self.server_identifier);
        msg.set_yiaddr(ip);

        msg.opts_mut()
            .insert(v4::DhcpOption::MessageType(MessageType::Offer));
        msg.opts_mut()
            .insert(v4::DhcpOption::ServerIdentifier(self.server_identifier));
        msg.opts_mut()
            .insert(v4::DhcpOption::AddressLeaseTime(network.lease_duration));

        // Set vendor class identifier based on client architecture
        let vendor_class = determine_vendor_class_identifier(req_ctx.client_arch);
        msg.opts_mut()
            .insert(v4::DhcpOption::ClassIdentifier(vendor_class.to_vec()));

        message_builder::add_network_options(&mut msg, network)?;

        self.detect_and_add_pxeboot_options(&mut msg, req_ctx, dev_ctx, network)
            .await?;

        Ok(msg)
    }

    async fn build_ack(
        &self,
        req: &Message,
        ip: Ipv4Addr,
        network: &DhcpNetwork,
        req_ctx: &RequestContext,
        dev_ctx: &DeviceContext,
    ) -> Result<Message> {
        let mut msg = self.build_offer(req, ip, network, req_ctx, dev_ctx).await?;
        msg.opts_mut()
            .insert(v4::DhcpOption::MessageType(MessageType::Ack));

        Ok(msg)
    }

    async fn detect_and_add_pxeboot_options(
        &self,
        msg: &mut Message,
        req_ctx: &RequestContext,
        dev_ctx: &DeviceContext,
        network: &DhcpNetwork,
    ) -> Result<()> {
        if !should_provide_boot_options(
            network.enable_autodiscovery,
            dev_ctx.device_uuid.as_ref(),
            dev_ctx.is_pending,
        ) {
            info!("Skipping boot options for unknown device (autodiscover disabled)");
            return Ok(());
        }

        self.boot_config.populate_boot_options(msg, req_ctx).await?;

        Ok(())
    }

    fn build_nak(&self, req: &Message) -> Result<Message> {
        Ok(message_builder::build_nak(req, self.server_identifier))
    }
}

#[cfg(test)]
mod tests {
    use crate::storage::MemoryImageStore;
    use chrono::DateTime;
    use dhcproto::v4::Opcode;

    use super::*;

    #[tokio::test]
    async fn test_server_identifier_in_offer() {
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
        let network = store.get_network(1).await.unwrap();

        // Create contexts
        let req_ctx = RequestContext::from_message(&discover);
        let dev_ctx = DeviceContext {
            device_uuid: None,
            is_disabled: false,
            disable_reason: None,
            is_pending: false,
        };

        // Build an OFFER response
        let offer = handler
            .build_offer(
                &discover,
                "10.0.0.100".parse().unwrap(),
                &network,
                &req_ctx,
                &dev_ctx,
            )
            .await
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

    #[tokio::test]
    async fn test_custom_server_identifier() {
        use super::super::device_resolution::DirectorDeviceResolver;
        use crate::boot_files::FilesystemBootFileProvider;
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

        let device_resolver = Arc::new(DirectorDeviceResolver::new(director));
        let allocator = IpAllocator::new(store.clone());

        // Create a temporary boot files directory for testing
        let boot_files_dir = temp_dir.path().join("boot_files");
        std::fs::create_dir_all(&boot_files_dir).unwrap();
        let boot_file_provider =
            Arc::new(FilesystemBootFileProvider::new(boot_files_dir.to_path_buf()).unwrap());

        let boot_config = BootConfigProvider::new(
            "10.0.0.1".to_string(),
            "http://10.0.0.1".to_string(),
            boot_file_provider,
        );

        // Use a custom server identifier different from gateway
        let custom_server_id: Ipv4Addr = "192.168.1.50".parse().unwrap();
        let handler = DhcpHandler::new(
            store.clone(),
            device_resolver,
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
        let network = store.get_network(1).await.unwrap();

        // Build an OFFER and verify it uses the custom identifier
        let mut discover = Message::default();
        discover.set_opcode(Opcode::BootRequest);
        discover.set_xid(0x12345678);
        discover.set_chaddr(&[0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff]);
        discover
            .opts_mut()
            .insert(v4::DhcpOption::MessageType(MessageType::Discover));

        // Create contexts
        let req_ctx = RequestContext::from_message(&discover);
        let dev_ctx = DeviceContext {
            device_uuid: None,
            is_disabled: false,
            disable_reason: None,
            is_pending: false,
        };

        let offer = handler
            .build_offer(
                &discover,
                "10.0.0.100".parse().unwrap(),
                &network,
                &req_ctx,
                &dev_ctx,
            )
            .await
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
        use super::super::device_resolution::DirectorDeviceResolver;
        use crate::boot_files::FilesystemBootFileProvider;
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

        let device_resolver = Arc::new(DirectorDeviceResolver::new(director));
        let allocator = IpAllocator::new(store.clone());

        // Create a temporary boot files directory for testing
        let boot_files_dir = temp_dir.path().join("boot_files");
        std::fs::create_dir_all(&boot_files_dir).unwrap();
        let boot_file_provider =
            Arc::new(FilesystemBootFileProvider::new(boot_files_dir.to_path_buf()).unwrap());

        let boot_config = BootConfigProvider::new(
            "10.0.0.1".to_string(),
            "http://10.0.0.1".to_string(),
            boot_file_provider,
        );
        let server_identifier = "10.0.0.1".parse().unwrap();

        let handler = DhcpHandler::new(
            store.clone(),
            device_resolver,
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

    // Authorization tests
    #[test]
    fn test_should_provide_boot_options_autodiscover_enabled() {
        assert!(
            should_provide_boot_options(true, None, false),
            "Should provide boot options when autodiscover is enabled"
        );
    }

    #[test]
    fn test_should_provide_boot_options_autodiscover_disabled_unknown_device() {
        assert!(
            !should_provide_boot_options(false, None, false),
            "Should NOT provide boot options for unknown device when autodiscover is disabled"
        );
    }

    #[test]
    fn test_should_provide_boot_options_known_device() {
        let uuid = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440001").unwrap();
        assert!(
            should_provide_boot_options(false, Some(&uuid), false),
            "Should provide boot options for known device even when autodiscover is disabled"
        );
    }

    #[test]
    fn test_should_provide_boot_options_pending_device() {
        assert!(
            should_provide_boot_options(false, None, true),
            "Should provide boot options for pending device even when autodiscover is disabled"
        );
    }

    #[tokio::test]
    async fn test_option_60_default_pxe() {
        let handler = create_test_handler();

        // Create a minimal REQUEST message without architecture
        let mut request = Message::default();
        request.set_opcode(Opcode::BootRequest);
        request.set_xid(0x12345678);
        request.set_chaddr(&[0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff]);
        request
            .opts_mut()
            .insert(v4::DhcpOption::MessageType(MessageType::Request));

        // Build an Offer response
        let network = DhcpNetwork {
            id: 1,
            name: String::default(),
            subnet: "255.255.255.1/24".to_string(),
            gateway: "255.255.255.1".to_string(),
            dns_servers: vec![],
            lease_duration: 1,
            relay_agent_address: None,
            enable_autodiscovery: true,
            created_at: DateTime::default(),
            updated_at: DateTime::default(),
        };
        let req_context = RequestContext::from_message(&request);
        let device_context = DeviceContext {
            device_uuid: None,
            is_disabled: false,
            disable_reason: None,
            is_pending: false,
        };
        let offer = handler
            .build_offer(
                &request,
                Ipv4Addr::UNSPECIFIED,
                &network,
                &req_context,
                &device_context,
            )
            .await
            .unwrap();

        // Verify the class identifier is PXEServer for default (no architecture)
        let class_ident = offer
            .opts()
            .iter()
            .find_map(|(_, opt)| {
                if let v4::DhcpOption::ClassIdentifier(class_ident) = opt {
                    Some(class_ident.clone())
                } else {
                    None
                }
            })
            .expect("Class Identifier should be present in OFFER");

        assert_eq!(
            class_ident, b"PXEServer",
            "Default (no architecture) should receive PXEServer vendor class identifier"
        );
    }

    // Vendor Class Identifier (Option 60) Tests
    #[test]
    fn test_determine_vendor_class_identifier_http_boot_arch_14() {
        let arch = Some(dhcproto::v4::Architecture::Unknown(14));
        assert_eq!(
            determine_vendor_class_identifier(arch),
            b"HTTPClient",
            "Architecture 14 (x86-64 HTTP) should return HTTPClient"
        );
    }

    #[test]
    fn test_determine_vendor_class_identifier_http_boot_arch_15() {
        let arch = Some(dhcproto::v4::Architecture::Unknown(15));
        assert_eq!(
            determine_vendor_class_identifier(arch),
            b"HTTPClient",
            "Architecture 15 (x86 HTTP/IA32) should return HTTPClient"
        );
    }

    #[test]
    fn test_determine_vendor_class_identifier_http_boot_arch_16() {
        let arch = Some(dhcproto::v4::Architecture::Unknown(16));
        assert_eq!(
            determine_vendor_class_identifier(arch),
            b"HTTPClient",
            "Architecture 16 (EBC HTTP) should return HTTPClient"
        );
    }

    #[test]
    fn test_determine_vendor_class_identifier_uefi_arch_7() {
        let arch = Some(dhcproto::v4::Architecture::BC);
        assert_eq!(
            determine_vendor_class_identifier(arch),
            b"PXEServer",
            "Architecture 7 (UEFI BC) should return PXEServer"
        );
    }

    #[test]
    fn test_determine_vendor_class_identifier_bios_arch_0() {
        let arch = Some(dhcproto::v4::Architecture::Intelx86PC);
        assert_eq!(
            determine_vendor_class_identifier(arch),
            b"PXEServer",
            "Architecture 0 (BIOS x86) should return PXEServer"
        );
    }

    #[test]
    fn test_determine_vendor_class_identifier_bios_arch_9() {
        let arch = Some(dhcproto::v4::Architecture::X86_64);
        assert_eq!(
            determine_vendor_class_identifier(arch),
            b"PXEServer",
            "Architecture 9 (BIOS x86-64) should return PXEServer"
        );
    }

    #[test]
    fn test_determine_vendor_class_identifier_no_arch() {
        let arch = None;
        assert_eq!(
            determine_vendor_class_identifier(arch),
            b"PXEServer",
            "No architecture should default to PXEServer"
        );
    }

    #[tokio::test]
    async fn test_vendor_class_identifier_in_offer_http_boot() {
        let (handler, store) = create_test_handler_with_store();

        // Create a DISCOVER message with HTTP boot architecture (14)
        let mut discover = Message::default();
        discover.set_opcode(Opcode::BootRequest);
        discover.set_xid(0x12345678);
        discover.set_chaddr(&[0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff]);
        discover
            .opts_mut()
            .insert(v4::DhcpOption::MessageType(MessageType::Discover));
        discover
            .opts_mut()
            .insert(v4::DhcpOption::ClientSystemArchitecture(
                dhcproto::v4::Architecture::Unknown(14),
            ));

        // Get default network
        let network = store.get_network(1).await.unwrap();

        // Create contexts
        let req_ctx = RequestContext::from_message(&discover);
        let dev_ctx = DeviceContext {
            device_uuid: None,
            is_disabled: false,
            disable_reason: None,
            is_pending: false,
        };

        // Build an OFFER response
        let offer = handler
            .build_offer(
                &discover,
                "10.0.0.100".parse().unwrap(),
                &network,
                &req_ctx,
                &dev_ctx,
            )
            .await
            .unwrap();

        // Verify the vendor class identifier is HTTPClient
        let class_ident = offer
            .opts()
            .iter()
            .find_map(|(_, opt)| {
                if let v4::DhcpOption::ClassIdentifier(class_ident) = opt {
                    Some(class_ident.clone())
                } else {
                    None
                }
            })
            .expect("Class Identifier should be present in OFFER");

        assert_eq!(
            class_ident, b"HTTPClient",
            "HTTP boot client (arch 14) should receive HTTPClient vendor class identifier"
        );
    }

    #[tokio::test]
    async fn test_vendor_class_identifier_in_offer_traditional_pxe() {
        let (handler, store) = create_test_handler_with_store();

        // Create a DISCOVER message with traditional UEFI architecture (7)
        let mut discover = Message::default();
        discover.set_opcode(Opcode::BootRequest);
        discover.set_xid(0x12345678);
        discover.set_chaddr(&[0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff]);
        discover
            .opts_mut()
            .insert(v4::DhcpOption::MessageType(MessageType::Discover));
        discover
            .opts_mut()
            .insert(v4::DhcpOption::ClientSystemArchitecture(
                dhcproto::v4::Architecture::BC,
            ));

        // Get default network
        let network = store.get_network(1).await.unwrap();

        // Create contexts
        let req_ctx = RequestContext::from_message(&discover);
        let dev_ctx = DeviceContext {
            device_uuid: None,
            is_disabled: false,
            disable_reason: None,
            is_pending: false,
        };

        // Build an OFFER response
        let offer = handler
            .build_offer(
                &discover,
                "10.0.0.100".parse().unwrap(),
                &network,
                &req_ctx,
                &dev_ctx,
            )
            .await
            .unwrap();

        // Verify the vendor class identifier is PXEServer
        let class_ident = offer
            .opts()
            .iter()
            .find_map(|(_, opt)| {
                if let v4::DhcpOption::ClassIdentifier(class_ident) = opt {
                    Some(class_ident.clone())
                } else {
                    None
                }
            })
            .expect("Class Identifier should be present in OFFER");

        assert_eq!(
            class_ident, b"PXEServer",
            "Traditional PXE client (arch 7) should receive PXEServer vendor class identifier"
        );
    }

    #[tokio::test]
    async fn test_vendor_class_identifier_in_ack_http_boot() {
        let (handler, store) = create_test_handler_with_store();

        // Create a REQUEST message with HTTP boot architecture (15)
        let mut request = Message::default();
        request.set_opcode(Opcode::BootRequest);
        request.set_xid(0x12345678);
        request.set_chaddr(&[0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff]);
        request
            .opts_mut()
            .insert(v4::DhcpOption::MessageType(MessageType::Request));
        request
            .opts_mut()
            .insert(v4::DhcpOption::ClientSystemArchitecture(
                dhcproto::v4::Architecture::Unknown(15),
            ));

        // Get default network
        let network = store.get_network(1).await.unwrap();

        // Create contexts
        let req_ctx = RequestContext::from_message(&request);
        let dev_ctx = DeviceContext {
            device_uuid: None,
            is_disabled: false,
            disable_reason: None,
            is_pending: false,
        };

        // Build an ACK response
        let ack = handler
            .build_ack(
                &request,
                "10.0.0.100".parse().unwrap(),
                &network,
                &req_ctx,
                &dev_ctx,
            )
            .await
            .unwrap();

        // Verify the vendor class identifier is HTTPClient
        let class_ident = ack
            .opts()
            .iter()
            .find_map(|(_, opt)| {
                if let v4::DhcpOption::ClassIdentifier(class_ident) = opt {
                    Some(class_ident.clone())
                } else {
                    None
                }
            })
            .expect("Class Identifier should be present in ACK");

        assert_eq!(
            class_ident, b"HTTPClient",
            "HTTP boot client (arch 15) should receive HTTPClient vendor class identifier in ACK"
        );
    }
}
