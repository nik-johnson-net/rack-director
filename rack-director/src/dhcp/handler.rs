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
use tokio_recvmsg::PktInfo;

use super::allocator;
use super::boot_config::BootConfigProvider;
use super::device_resolution::{DeviceContext, DeviceResolver};
use super::interface;
use super::message_builder;
use super::request::{RequestContext, extract_server_identifier};
use super::store::{self, DhcpNetwork, LeaseState, format_mac};
use crate::database::{Connection, ConnectionFactory};

/// Reply to send after processing a DHCP packet.
pub enum DhcpReply {
    /// L2 client response. `local_ip` selects which per-network socket to send from.
    /// `peer_addr` is the source address of the received packet — used to decide
    /// whether to broadcast (peer IP is unspecified) or unicast (peer IP is set, e.g. test/renewal).
    L2 {
        data: Vec<u8>,
        local_ip: Ipv4Addr,
        peer_addr: SocketAddr,
    },
    /// Relay agent response — unicast to the relay agent on port 67.
    Relay { data: Vec<u8>, dest: SocketAddr },
}

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

#[derive(Clone)]
pub struct DhcpHandler {
    db: Arc<dyn ConnectionFactory>,
    device_resolver: Arc<dyn DeviceResolver>,
    boot_config: BootConfigProvider,
    server_identifier: Ipv4Addr,
}

impl DhcpHandler {
    pub fn new(
        db: Arc<dyn ConnectionFactory>,
        device_resolver: Arc<dyn DeviceResolver>,
        boot_config: BootConfigProvider,
        server_identifier: Ipv4Addr,
    ) -> Self {
        Self {
            db,
            device_resolver,
            boot_config,
            server_identifier,
        }
    }

    /// Handle a DHCP packet received on the wildcard broadcast socket.
    ///
    /// Uses the `PktInfo` (interface index and destination address) from recvmsg to identify
    /// which L2 network the packet belongs to. For relay-agent packets (giaddr != 0), falls
    /// back to relay-based network selection.
    pub async fn handle_packet(
        &self,
        data: &[u8],
        pkt_info: &PktInfo,
    ) -> Result<Option<DhcpReply>> {
        let msg = match Message::decode(&mut Decoder::new(data)) {
            Ok(msg) => msg,
            Err(e) => {
                log::warn!("Failed to decode DHCP message: {}", e);
                return Ok(None);
            }
        };

        trace!("DHCP: Received packet {:?}", msg);
        let conn = self.db.open().await?;

        // If relay agent (giaddr != 0), use relay-based network selection
        if msg.giaddr() != Ipv4Addr::UNSPECIFIED {
            let relay_agent = msg.giaddr();
            let network = match store::get_network_by_relay(&conn, Some(relay_agent)).await? {
                Some(n) => n,
                None => {
                    log::warn!("No network found for relay agent {}", relay_agent);
                    return Ok(None);
                }
            };
            debug!(
                "Using network '{}' (id={}) for relay {}",
                network.name, network.id, relay_agent
            );
            let dest = SocketAddr::new(relay_agent.into(), 67);
            return self
                .process_and_reply(&conn, &msg, &network, self.server_identifier, move |data| {
                    DhcpReply::Relay { data, dest }
                })
                .await;
        }

        // L2 path: use interface index to find matching network
        let l2_networks = store::get_l2_networks(&conn).await?;
        let Some((network, local_ip)) =
            interface::find_matching_l2_network(pkt_info.if_index, &l2_networks)?
        else {
            debug!(
                "No L2 network matches interface {}, dropping",
                pkt_info.if_index
            );
            return Ok(None);
        };
        debug!(
            "Using network '{}' (id={}) for interface index {} (local_ip={})",
            network.name, network.id, pkt_info.if_index, local_ip
        );
        let peer_addr = pkt_info.addr_src;
        self.process_and_reply(&conn, &msg, network, local_ip, move |data| DhcpReply::L2 {
            data,
            local_ip,
            peer_addr,
        })
        .await
    }

    /// Handle a DHCP packet received on a per-network socket (unicast renewals).
    ///
    /// The `local_ip` is the address the per-network socket is bound to, which identifies
    /// which L2 network this packet belongs to.
    pub async fn handle_l2_unicast_packet(
        &self,
        data: &[u8],
        peer_addr: SocketAddr,
        local_ip: Ipv4Addr,
    ) -> Result<Option<DhcpReply>> {
        let msg = match Message::decode(&mut Decoder::new(data)) {
            Ok(msg) => msg,
            Err(e) => {
                log::warn!("Failed to decode DHCP message: {}", e);
                return Ok(None);
            }
        };

        trace!("DHCP unicast: Received packet {:?}", msg);
        let conn = self.db.open().await?;

        let l2_networks = store::get_l2_networks(&conn).await?;
        let Some(network) = interface::find_l2_network_for_ip(local_ip, &l2_networks)? else {
            debug!("No L2 network matches local IP {}, dropping", local_ip);
            return Ok(None);
        };
        debug!(
            "Unicast: Using network '{}' (id={}) for local_ip={}",
            network.name, network.id, local_ip
        );
        self.process_and_reply(&conn, &msg, network, local_ip, move |data| DhcpReply::L2 {
            data,
            local_ip,
            peer_addr,
        })
        .await
    }

    /// Shared logic for processing a DHCP message against a selected network and producing a reply.
    async fn process_and_reply<F>(
        &self,
        conn: &Connection,
        msg: &Message,
        network: &DhcpNetwork,
        server_identifier: Ipv4Addr,
        make_reply: F,
    ) -> Result<Option<DhcpReply>>
    where
        F: FnOnce(Vec<u8>) -> DhcpReply,
    {
        let response = match msg.opts().msg_type() {
            Some(MessageType::Discover) => {
                self.handle_discover(conn, msg, network, server_identifier)
                    .await?
            }
            Some(MessageType::Request) => {
                self.handle_request(conn, msg, network, server_identifier)
                    .await?
            }
            Some(MessageType::Release) => {
                self.handle_release(conn, msg).await?;
                None
            }
            Some(MessageType::Decline) => {
                self.handle_decline(conn, msg).await?;
                None
            }
            _ => {
                log::debug!("Ignoring unsupported DHCP message type");
                return Ok(None);
            }
        };

        if let Some(resp) = response {
            trace!("DHCP: Sending response {:?}", resp);
            let mut buf = Vec::new();
            resp.encode(&mut Encoder::new(&mut buf))?;
            Ok(Some(make_reply(buf)))
        } else {
            Ok(None)
        }
    }

    async fn handle_discover(
        &self,
        conn: &Connection,
        msg: &Message,
        network: &DhcpNetwork,
        server_identifier: Ipv4Addr,
    ) -> Result<Option<Message>> {
        let req_ctx = RequestContext::from_message(msg);

        info!(
            "DHCP DISCOVER from MAC {} on network '{}'{}",
            req_ctx.mac,
            network.name,
            req_ctx
                .guid
                .map(|g| format!(" (GUID: {})", g))
                .unwrap_or_default()
        );

        let dev_ctx = self
            .device_resolver
            .resolve(conn, &req_ctx.mac, req_ctx.guid.as_ref())
            .await?;

        if dev_ctx.is_disabled {
            warn!(
                "Skipping DHCP DISCOVER for disabled interface {} on device {}. Reason: {}",
                req_ctx.mac,
                dev_ctx
                    .device_uuid
                    .as_ref()
                    .map(|u: &uuid::Uuid| u.to_string())
                    .unwrap_or_default(),
                dev_ctx.disable_reason.as_deref().unwrap_or("unknown")
            );
            return Ok(None);
        }

        // Allocate or retrieve existing IP in this network
        let ip = if let Some(uuid) = &dev_ctx.device_uuid {
            debug!("Device UUID {} found for MAC {}", uuid, req_ctx.mac);
            allocator::allocate_for_device_in_network(conn, &req_ctx.mac, uuid, network.id).await?
        } else {
            debug!(
                "No device UUID found for MAC {}, allocating from pool",
                req_ctx.mac
            );
            allocator::allocate_for_mac_in_network(conn, &req_ctx.mac, network.id).await?
        };

        // Create lease in 'offered' state
        store::create_or_update_lease_with_network(
            conn,
            &req_ctx.mac,
            &ip,
            dev_ctx.device_uuid.as_ref(),
            LeaseState::Offered,
            network.lease_duration,
            network.id,
        )
        .await?;

        let offer = self
            .build_offer(msg, ip, network, &req_ctx, &dev_ctx, server_identifier)
            .await?;
        info!(
            "DHCP OFFER {} to MAC {} on network '{}'",
            ip, req_ctx.mac, network.name
        );

        Ok(Some(offer))
    }

    async fn handle_request(
        &self,
        conn: &Connection,
        msg: &Message,
        network: &DhcpNetwork,
        server_identifier: Ipv4Addr,
    ) -> Result<Option<Message>> {
        let req_ctx = RequestContext::from_message(msg);

        // Check Server Identifier option (Option 54) per RFC 2131 Section 4.3.2
        // If present, only respond if it matches our server identifier
        if let Some(server_id) = extract_server_identifier(msg)
            && server_id != server_identifier
        {
            // This request is for a different DHCP server - ignore it
            debug!(
                "Ignoring DHCPREQUEST from {} - server identifier {} doesn't match ours {}",
                req_ctx.mac, server_id, server_identifier
            );
            return Ok(None);
        }
        // Note: If no Server Identifier is present, this is an INIT-REBOOT, RENEWING,
        // or REBINDING request and should be processed normally per RFC 2131

        info!(
            "DHCP REQUEST from MAC {} on network '{}'{}",
            req_ctx.mac,
            network.name,
            req_ctx
                .guid
                .map(|g| format!(" (GUID: {})", g))
                .unwrap_or_default()
        );

        let dev_ctx = self
            .device_resolver
            .resolve(conn, &req_ctx.mac, req_ctx.guid.as_ref())
            .await?;

        if dev_ctx.is_disabled {
            warn!(
                "Skipping DHCP REQUEST for disabled interface {} on device {}. Reason: {}",
                req_ctx.mac,
                dev_ctx
                    .device_uuid
                    .as_ref()
                    .map(|u: &uuid::Uuid| u.to_string())
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
            return Ok(Some(self.build_nak(msg, server_identifier)?));
        };

        debug!("Requested IP: {}", requested_ip);

        // Check for static reservation - takes priority over everything
        let static_reservation =
            store::get_static_reservation(conn, network.id, &req_ctx.mac).await?;

        if let Some(reservation) = &static_reservation {
            let reserved_ip: Ipv4Addr = reservation.ip_address.parse()?;

            // If client is requesting a different IP than the static reservation, NAK it
            if requested_ip != reserved_ip {
                warn!(
                    "NAKing DHCPREQUEST from {} - requested {} but static reservation is {}",
                    req_ctx.mac, requested_ip, reserved_ip
                );
                return Ok(Some(self.build_nak(msg, server_identifier)?));
            }

            // Static reservation matches requested IP - update or create lease
            store::create_or_update_lease_with_network(
                conn,
                &req_ctx.mac,
                &reserved_ip,
                dev_ctx.device_uuid.as_ref(),
                LeaseState::Active,
                network.lease_duration,
                network.id,
            )
            .await?;

            if let Some(uuid) = &dev_ctx.device_uuid {
                self.device_resolver
                    .on_lease_activated(conn, uuid, &reserved_ip.to_string(), &req_ctx.mac)
                    .await?;
            }

            let ack = self
                .build_ack(
                    msg,
                    reserved_ip,
                    network,
                    &req_ctx,
                    &dev_ctx,
                    server_identifier,
                )
                .await?;
            info!(
                "DHCP ACK {} to MAC {} on network '{}' (static reservation)",
                reserved_ip, req_ctx.mac, network.name
            );

            return Ok(Some(ack));
        }

        // No static reservation - validate request matches our offer
        let lease = store::get_lease_by_mac(conn, &req_ctx.mac).await?;
        if let Some(lease) = lease {
            let lease_ip: Ipv4Addr = lease.ip_address.parse()?;
            if lease_ip != requested_ip {
                warn!(
                    "DHCP REQUEST IP mismatch: requested {}, expected {}",
                    requested_ip, lease_ip
                );
                return Ok(Some(self.build_nak(msg, server_identifier)?));
            }

            // Update lease to 'active'
            store::activate_lease(conn, &req_ctx.mac).await?;
            if let Some(uuid) = &dev_ctx.device_uuid {
                self.device_resolver
                    .on_lease_activated(conn, uuid, &lease_ip.to_string(), &req_ctx.mac)
                    .await?;
            }

            let ack = self
                .build_ack(
                    msg,
                    lease_ip,
                    network,
                    &req_ctx,
                    &dev_ctx,
                    server_identifier,
                )
                .await?;
            info!(
                "DHCP ACK {} to MAC {} on network '{}'",
                requested_ip, req_ctx.mac, network.name
            );

            Ok(Some(ack))
        } else {
            warn!("No lease found for MAC {}", req_ctx.mac);
            Ok(Some(self.build_nak(msg, server_identifier)?))
        }
    }

    async fn handle_release(&self, conn: &Connection, msg: &Message) -> Result<()> {
        let mac = msg.chaddr();
        let mac_str = format_mac(mac);

        info!("DHCP RELEASE from MAC {}", mac_str);

        store::release_lease(conn, &mac_str).await?;

        Ok(())
    }

    async fn handle_decline(&self, conn: &Connection, msg: &Message) -> Result<()> {
        let mac = msg.chaddr();
        let mac_str = format_mac(mac);

        warn!("DHCP DECLINE from MAC {}", mac_str);

        // Mark lease as released to prevent reuse
        store::release_lease(conn, &mac_str).await?;

        Ok(())
    }

    async fn build_offer(
        &self,
        req: &Message,
        ip: Ipv4Addr,
        network: &DhcpNetwork,
        req_ctx: &RequestContext,
        _dev_ctx: &DeviceContext,
        server_identifier: Ipv4Addr,
    ) -> Result<Message> {
        let mut msg = message_builder::create_base_reply(req, &server_identifier);
        msg.set_yiaddr(ip);

        msg.opts_mut()
            .insert(v4::DhcpOption::MessageType(MessageType::Offer));
        msg.opts_mut()
            .insert(v4::DhcpOption::ServerIdentifier(server_identifier));
        msg.opts_mut()
            .insert(v4::DhcpOption::AddressLeaseTime(network.lease_duration));

        // Set vendor class identifier based on client architecture
        let vendor_class = determine_vendor_class_identifier(req_ctx.client_arch);
        msg.opts_mut()
            .insert(v4::DhcpOption::ClassIdentifier(vendor_class.to_vec()));

        message_builder::add_network_options(&mut msg, network)?;

        self.boot_config
            .populate_boot_options(&mut msg, req_ctx)
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
        server_identifier: Ipv4Addr,
    ) -> Result<Message> {
        let mut msg = self
            .build_offer(req, ip, network, req_ctx, dev_ctx, server_identifier)
            .await?;
        msg.opts_mut()
            .insert(v4::DhcpOption::MessageType(MessageType::Ack));

        Ok(msg)
    }

    fn build_nak(&self, req: &Message, server_identifier: Ipv4Addr) -> Result<Message> {
        Ok(message_builder::build_nak(req, server_identifier))
    }
}

#[cfg(test)]
mod tests {
    use chrono::DateTime;
    use dhcproto::v4::Opcode;

    use super::*;
    use crate::test_connection_factory;

    #[tokio::test]
    async fn test_server_identifier_in_offer() {
        let (handler, conn, network_id, _temp_dir) =
            create_test_handler_with_store(test_connection_factory!()).await;

        // Create a minimal DISCOVER message
        let mut discover = Message::default();
        discover.set_opcode(Opcode::BootRequest);
        discover.set_xid(0x12345678);
        discover.set_chaddr(&[0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff]);
        discover
            .opts_mut()
            .insert(v4::DhcpOption::MessageType(MessageType::Discover));

        // Get test network
        let network = store::get_network(&conn, network_id).await.unwrap();

        // Create contexts
        let req_ctx = RequestContext::from_message(&discover);
        let dev_ctx = DeviceContext {
            device_uuid: None,
            is_disabled: false,
            disable_reason: None,
        };

        // Build an OFFER response
        let offer = handler
            .build_offer(
                &discover,
                "10.0.0.100".parse().unwrap(),
                &network,
                &req_ctx,
                &dev_ctx,
                handler.server_identifier,
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

    #[tokio::test]
    async fn test_server_identifier_in_nak() {
        let handler = create_test_handler(test_connection_factory!()).await;

        // Create a minimal REQUEST message
        let mut request = Message::default();
        request.set_opcode(Opcode::BootRequest);
        request.set_xid(0x12345678);
        request.set_chaddr(&[0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff]);
        request
            .opts_mut()
            .insert(v4::DhcpOption::MessageType(MessageType::Request));

        // Build a NAK response
        let nak = handler
            .build_nak(&request, handler.server_identifier)
            .unwrap();

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
        use crate::database::DatabaseConnectionFactory;
        use std::sync::Arc;
        use tempfile::tempdir;

        let temp_dir = tempdir().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let factory: Arc<dyn crate::database::ConnectionFactory> =
            Arc::new(DatabaseConnectionFactory::new(db_path));
        let conn = crate::database::run_migrations(factory.as_ref())
            .await
            .unwrap();

        // Create test network
        let network = store::create_network(
            &conn,
            "Test Network",
            "10.0.0.0/24",
            "10.0.0.1",
            &["8.8.8.8".to_string(), "8.8.4.4".to_string()],
            86400,
            None,
            false,
        )
        .await
        .unwrap();

        // Create test pool
        store::create_pool(&conn, network.id, "Test Pool", "10.0.0.100", "10.0.0.200")
            .await
            .unwrap();

        let device_resolver = Arc::new(DirectorDeviceResolver::new());

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
        let handler = DhcpHandler::new(factory, device_resolver, boot_config, custom_server_id);

        // Verify the handler stores the custom value
        assert_eq!(
            handler.server_identifier, custom_server_id,
            "Handler should store custom server identifier"
        );

        // Get the test network we just created
        let network = store::get_network(&conn, network.id).await.unwrap();

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
        };

        let offer = handler
            .build_offer(
                &discover,
                "10.0.0.100".parse().unwrap(),
                &network,
                &req_ctx,
                &dev_ctx,
                custom_server_id,
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

    async fn create_test_handler_with_store(
        factory: crate::database::DatabaseConnectionFactory,
    ) -> (
        DhcpHandler,
        crate::database::Connection,
        i64,
        tempfile::TempDir,
    ) {
        use super::super::device_resolution::DirectorDeviceResolver;
        use crate::boot_files::FilesystemBootFileProvider;
        use crate::database;
        use std::sync::Arc;
        use tempfile::tempdir;

        let factory: Arc<dyn crate::database::ConnectionFactory> = Arc::new(factory);
        // Run migrations and get connection for setup and test assertions
        let conn = database::run_migrations(factory.as_ref()).await.unwrap();

        // Create test network
        let network = store::create_network(
            &conn,
            "Test Network",
            "10.0.0.0/24",
            "10.0.0.1",
            &["8.8.8.8".to_string(), "8.8.4.4".to_string()],
            86400,
            None,
            false,
        )
        .await
        .unwrap();

        // Create test pool
        store::create_pool(&conn, network.id, "Test Pool", "10.0.0.100", "10.0.0.200")
            .await
            .unwrap();

        let device_resolver = Arc::new(DirectorDeviceResolver::new());

        // Create a temporary boot files directory for testing
        let temp_dir = tempdir().unwrap();
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

        let handler = DhcpHandler::new(factory, device_resolver, boot_config, server_identifier);
        (handler, conn, network.id, temp_dir)
    }

    async fn create_test_handler(
        factory: crate::database::DatabaseConnectionFactory,
    ) -> DhcpHandler {
        let (handler, _db, _network_id, _temp_dir) = create_test_handler_with_store(factory).await;
        handler
    }

    #[tokio::test]
    async fn test_option_60_default_pxe() {
        let handler = create_test_handler(test_connection_factory!()).await;

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
        };
        let offer = handler
            .build_offer(
                &request,
                Ipv4Addr::UNSPECIFIED,
                &network,
                &req_context,
                &device_context,
                handler.server_identifier,
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
        let (handler, conn, network_id, _temp_dir) =
            create_test_handler_with_store(test_connection_factory!()).await;

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

        // Get test network
        let network = store::get_network(&conn, network_id).await.unwrap();

        // Create contexts
        let req_ctx = RequestContext::from_message(&discover);
        let dev_ctx = DeviceContext {
            device_uuid: None,
            is_disabled: false,
            disable_reason: None,
        };

        // Build an OFFER response
        let offer = handler
            .build_offer(
                &discover,
                "10.0.0.100".parse().unwrap(),
                &network,
                &req_ctx,
                &dev_ctx,
                handler.server_identifier,
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
        let (handler, conn, network_id, _temp_dir) =
            create_test_handler_with_store(test_connection_factory!()).await;

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

        // Get test network
        let network = store::get_network(&conn, network_id).await.unwrap();

        // Create contexts
        let req_ctx = RequestContext::from_message(&discover);
        let dev_ctx = DeviceContext {
            device_uuid: None,
            is_disabled: false,
            disable_reason: None,
        };

        // Build an OFFER response
        let offer = handler
            .build_offer(
                &discover,
                "10.0.0.100".parse().unwrap(),
                &network,
                &req_ctx,
                &dev_ctx,
                handler.server_identifier,
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
        let (handler, conn, network_id, _temp_dir) =
            create_test_handler_with_store(test_connection_factory!()).await;

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

        // Get test network
        let network = store::get_network(&conn, network_id).await.unwrap();

        // Create contexts
        let req_ctx = RequestContext::from_message(&request);
        let dev_ctx = DeviceContext {
            device_uuid: None,
            is_disabled: false,
            disable_reason: None,
        };

        // Build an ACK response
        let ack = handler
            .build_ack(
                &request,
                "10.0.0.100".parse().unwrap(),
                &network,
                &req_ctx,
                &dev_ctx,
                handler.server_identifier,
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

    // Server Identifier Matching Tests

    #[tokio::test]
    async fn test_handle_request_matching_server_id() {
        let (handler, conn, network_id, _temp_dir) =
            create_test_handler_with_store(test_connection_factory!()).await;

        // Create a lease first
        let mac = "aa:bb:cc:dd:ee:ff";
        let ip: Ipv4Addr = "10.0.0.100".parse().unwrap();
        store::create_or_update_lease_with_network(
            &conn,
            mac,
            &ip,
            None,
            LeaseState::Offered,
            3600,
            network_id,
        )
        .await
        .unwrap();

        // Create a REQUEST message with matching server ID
        let mut request = Message::default();
        request.set_opcode(Opcode::BootRequest);
        request.set_xid(0x12345678);
        request.set_chaddr(&[0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff]);
        request
            .opts_mut()
            .insert(v4::DhcpOption::MessageType(MessageType::Request));
        request
            .opts_mut()
            .insert(v4::DhcpOption::RequestedIpAddress(ip));
        request
            .opts_mut()
            .insert(v4::DhcpOption::ServerIdentifier(handler.server_identifier));

        // Get test network
        let network = store::get_network(&conn, network_id).await.unwrap();

        // Handle the request
        let response = handler
            .handle_request(&conn, &request, &network, handler.server_identifier)
            .await
            .unwrap();

        // Should receive an ACK
        assert!(
            response.is_some(),
            "Should process request with matching server ID"
        );

        let ack = response.unwrap();
        let msg_type = ack
            .opts()
            .iter()
            .find_map(|(_, opt)| {
                if let v4::DhcpOption::MessageType(mt) = opt {
                    Some(*mt)
                } else {
                    None
                }
            })
            .expect("Message type should be present");

        assert_eq!(msg_type, MessageType::Ack, "Should respond with ACK");
    }

    #[tokio::test]
    async fn test_handle_request_non_matching_server_id() {
        let (handler, conn, network_id, _temp_dir) =
            create_test_handler_with_store(test_connection_factory!()).await;

        // Create a lease first
        let mac = "aa:bb:cc:dd:ee:ff";
        let ip: Ipv4Addr = "10.0.0.100".parse().unwrap();
        store::create_or_update_lease_with_network(
            &conn,
            mac,
            &ip,
            None,
            LeaseState::Offered,
            3600,
            network_id,
        )
        .await
        .unwrap();

        // Create a REQUEST message with non-matching server ID
        let wrong_server_id: Ipv4Addr = "192.168.1.99".parse().unwrap();
        assert_ne!(
            wrong_server_id, handler.server_identifier,
            "Test setup: wrong server ID should differ from handler's server ID"
        );

        let mut request = Message::default();
        request.set_opcode(Opcode::BootRequest);
        request.set_xid(0x12345678);
        request.set_chaddr(&[0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff]);
        request
            .opts_mut()
            .insert(v4::DhcpOption::MessageType(MessageType::Request));
        request
            .opts_mut()
            .insert(v4::DhcpOption::RequestedIpAddress(ip));
        request
            .opts_mut()
            .insert(v4::DhcpOption::ServerIdentifier(wrong_server_id));

        // Get test network
        let network = store::get_network(&conn, network_id).await.unwrap();

        // Handle the request
        let response = handler
            .handle_request(&conn, &request, &network, handler.server_identifier)
            .await
            .unwrap();

        // Should NOT respond (silently ignore)
        assert!(
            response.is_none(),
            "Should silently ignore request with non-matching server ID"
        );
    }

    #[tokio::test]
    async fn test_handle_request_without_server_id() {
        let (handler, conn, network_id, _temp_dir) =
            create_test_handler_with_store(test_connection_factory!()).await;

        // Create a lease first
        let mac = "aa:bb:cc:dd:ee:ff";
        let ip: Ipv4Addr = "10.0.0.100".parse().unwrap();
        store::create_or_update_lease_with_network(
            &conn,
            mac,
            &ip,
            None,
            LeaseState::Offered,
            3600,
            network_id,
        )
        .await
        .unwrap();

        // Create a REQUEST message WITHOUT server ID (RENEWING or INIT-REBOOT)
        let mut request = Message::default();
        request.set_opcode(Opcode::BootRequest);
        request.set_xid(0x12345678);
        request.set_chaddr(&[0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff]);
        request
            .opts_mut()
            .insert(v4::DhcpOption::MessageType(MessageType::Request));
        request
            .opts_mut()
            .insert(v4::DhcpOption::RequestedIpAddress(ip));
        // Note: No ServerIdentifier option added

        // Get test network
        let network = store::get_network(&conn, network_id).await.unwrap();

        // Handle the request
        let response = handler
            .handle_request(&conn, &request, &network, handler.server_identifier)
            .await
            .unwrap();

        // Should process the request (RENEWING/REBINDING case)
        assert!(
            response.is_some(),
            "Should process request without server ID (RENEWING/REBINDING)"
        );

        let ack = response.unwrap();
        let msg_type = ack
            .opts()
            .iter()
            .find_map(|(_, opt)| {
                if let v4::DhcpOption::MessageType(mt) = opt {
                    Some(*mt)
                } else {
                    None
                }
            })
            .expect("Message type should be present");

        assert_eq!(msg_type, MessageType::Ack, "Should respond with ACK");
    }

    #[tokio::test]
    async fn test_handle_request_init_reboot_without_server_id() {
        let (handler, conn, network_id, _temp_dir) =
            create_test_handler_with_store(test_connection_factory!()).await;

        // Create a lease first
        let mac = "aa:bb:cc:dd:ee:ff";
        let ip: Ipv4Addr = "10.0.0.100".parse().unwrap();
        store::create_or_update_lease_with_network(
            &conn,
            mac,
            &ip,
            None,
            LeaseState::Active,
            3600,
            network_id,
        )
        .await
        .unwrap();

        // Create an INIT-REBOOT REQUEST (has requested IP, no server ID, no ciaddr)
        let mut request = Message::default();
        request.set_opcode(Opcode::BootRequest);
        request.set_xid(0x12345678);
        request.set_chaddr(&[0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff]);
        request.set_ciaddr(Ipv4Addr::UNSPECIFIED); // No ciaddr in INIT-REBOOT
        request
            .opts_mut()
            .insert(v4::DhcpOption::MessageType(MessageType::Request));
        request
            .opts_mut()
            .insert(v4::DhcpOption::RequestedIpAddress(ip));
        // No ServerIdentifier - characteristic of INIT-REBOOT

        // Get test network
        let network = store::get_network(&conn, network_id).await.unwrap();

        // Handle the request
        let response = handler
            .handle_request(&conn, &request, &network, handler.server_identifier)
            .await
            .unwrap();

        // Should process INIT-REBOOT request
        assert!(
            response.is_some(),
            "Should process INIT-REBOOT request (no server ID)"
        );

        let ack = response.unwrap();
        let msg_type = ack
            .opts()
            .iter()
            .find_map(|(_, opt)| {
                if let v4::DhcpOption::MessageType(mt) = opt {
                    Some(*mt)
                } else {
                    None
                }
            })
            .expect("Message type should be present");

        assert_eq!(
            msg_type,
            MessageType::Ack,
            "Should respond with ACK to INIT-REBOOT"
        );
    }

    // Static Reservation Tests

    #[tokio::test]
    async fn test_static_reservation_nak_on_wrong_requested_ip() {
        let (handler, conn, network_id, _temp_dir) =
            create_test_handler_with_store(test_connection_factory!()).await;

        let mac = "aa:bb:cc:dd:ee:ff";
        let reserved_ip: Ipv4Addr = "10.0.0.50".parse().unwrap();
        let requested_ip: Ipv4Addr = "10.0.0.100".parse().unwrap();

        // Create static reservation
        store::create_static_reservation(&conn, network_id, mac, &reserved_ip.to_string(), None)
            .await
            .unwrap();

        // Create a REQUEST with different IP than reservation
        let mut request = Message::default();
        request.set_opcode(Opcode::BootRequest);
        request.set_xid(0x12345678);
        request.set_chaddr(&[0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff]);
        request
            .opts_mut()
            .insert(v4::DhcpOption::MessageType(MessageType::Request));
        request
            .opts_mut()
            .insert(v4::DhcpOption::RequestedIpAddress(requested_ip));

        let network = store::get_network(&conn, network_id).await.unwrap();
        let response = handler
            .handle_request(&conn, &request, &network, handler.server_identifier)
            .await
            .unwrap();

        // Should receive NAK
        assert!(response.is_some(), "Should respond with NAK");

        let nak = response.unwrap();
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
            .expect("Message type should be present");

        assert_eq!(
            msg_type,
            MessageType::Nak,
            "Should NAK request for wrong IP when static reservation exists"
        );
    }

    #[tokio::test]
    async fn test_static_reservation_nak_on_wrong_ciaddr() {
        let (handler, conn, network_id, _temp_dir) =
            create_test_handler_with_store(test_connection_factory!()).await;

        let mac = "aa:bb:cc:dd:ee:ff";
        let reserved_ip: Ipv4Addr = "10.0.0.50".parse().unwrap();
        let ciaddr: Ipv4Addr = "10.0.0.100".parse().unwrap();

        // Create static reservation
        store::create_static_reservation(&conn, network_id, mac, &reserved_ip.to_string(), None)
            .await
            .unwrap();

        // Create a renewal REQUEST (with ciaddr) for different IP
        let mut request = Message::default();
        request.set_opcode(Opcode::BootRequest);
        request.set_xid(0x12345678);
        request.set_chaddr(&[0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff]);
        request.set_ciaddr(ciaddr); // Client renewing with wrong IP
        request
            .opts_mut()
            .insert(v4::DhcpOption::MessageType(MessageType::Request));

        let network = store::get_network(&conn, network_id).await.unwrap();
        let response = handler
            .handle_request(&conn, &request, &network, handler.server_identifier)
            .await
            .unwrap();

        // Should receive NAK
        assert!(response.is_some(), "Should respond with NAK");

        let nak = response.unwrap();
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
            .expect("Message type should be present");

        assert_eq!(
            msg_type,
            MessageType::Nak,
            "Should NAK renewal with wrong ciaddr when static reservation exists"
        );
    }

    #[tokio::test]
    async fn test_static_reservation_ack_on_correct_ip() {
        let (handler, conn, network_id, _temp_dir) =
            create_test_handler_with_store(test_connection_factory!()).await;

        let mac = "aa:bb:cc:dd:ee:ff";
        let reserved_ip: Ipv4Addr = "10.0.0.50".parse().unwrap();

        // Create static reservation
        store::create_static_reservation(&conn, network_id, mac, &reserved_ip.to_string(), None)
            .await
            .unwrap();

        // Create a REQUEST with matching IP
        let mut request = Message::default();
        request.set_opcode(Opcode::BootRequest);
        request.set_xid(0x12345678);
        request.set_chaddr(&[0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff]);
        request
            .opts_mut()
            .insert(v4::DhcpOption::MessageType(MessageType::Request));
        request
            .opts_mut()
            .insert(v4::DhcpOption::RequestedIpAddress(reserved_ip));

        let network = store::get_network(&conn, network_id).await.unwrap();
        let response = handler
            .handle_request(&conn, &request, &network, handler.server_identifier)
            .await
            .unwrap();

        // Should receive ACK
        assert!(response.is_some(), "Should respond with ACK");

        let ack = response.unwrap();
        let msg_type = ack
            .opts()
            .iter()
            .find_map(|(_, opt)| {
                if let v4::DhcpOption::MessageType(mt) = opt {
                    Some(*mt)
                } else {
                    None
                }
            })
            .expect("Message type should be present");

        assert_eq!(
            msg_type,
            MessageType::Ack,
            "Should ACK request with correct reserved IP"
        );

        // Verify the ACK contains the reserved IP
        assert_eq!(
            ack.yiaddr(),
            reserved_ip,
            "ACK should contain the reserved IP"
        );
    }

    #[tokio::test]
    async fn test_static_reservation_overrides_existing_lease() {
        let (handler, conn, network_id, _temp_dir) =
            create_test_handler_with_store(test_connection_factory!()).await;

        let mac = "aa:bb:cc:dd:ee:ff";
        let old_ip: Ipv4Addr = "10.0.0.100".parse().unwrap();
        let reserved_ip: Ipv4Addr = "10.0.0.50".parse().unwrap();

        // Create an active lease for the old IP
        store::create_or_update_lease_with_network(
            &conn,
            mac,
            &old_ip,
            None,
            LeaseState::Active,
            3600,
            network_id,
        )
        .await
        .unwrap();

        // Admin creates static reservation for different IP
        store::create_static_reservation(&conn, network_id, mac, &reserved_ip.to_string(), None)
            .await
            .unwrap();

        // Client tries to renew the old IP
        let mut request = Message::default();
        request.set_opcode(Opcode::BootRequest);
        request.set_xid(0x12345678);
        request.set_chaddr(&[0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff]);
        request.set_ciaddr(old_ip); // Renewing old IP
        request
            .opts_mut()
            .insert(v4::DhcpOption::MessageType(MessageType::Request));

        let network = store::get_network(&conn, network_id).await.unwrap();
        let response = handler
            .handle_request(&conn, &request, &network, handler.server_identifier)
            .await
            .unwrap();

        // Should receive NAK because old IP doesn't match static reservation
        assert!(response.is_some(), "Should respond with NAK");

        let nak = response.unwrap();
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
            .expect("Message type should be present");

        assert_eq!(
            msg_type,
            MessageType::Nak,
            "Should NAK renewal when static reservation changes to different IP"
        );
    }

    #[tokio::test]
    async fn test_static_reservation_full_workflow() {
        let (handler, conn, network_id, _temp_dir) =
            create_test_handler_with_store(test_connection_factory!()).await;

        let mac = "aa:bb:cc:dd:ee:ff";
        let old_ip: Ipv4Addr = "10.0.0.100".parse().unwrap();
        let reserved_ip: Ipv4Addr = "10.0.0.50".parse().unwrap();

        // Step 1: Client gets IP from pool
        store::create_or_update_lease_with_network(
            &conn,
            mac,
            &old_ip,
            None,
            LeaseState::Active,
            3600,
            network_id,
        )
        .await
        .unwrap();

        // Step 2: Admin creates static reservation for different IP
        store::create_static_reservation(&conn, network_id, mac, &reserved_ip.to_string(), None)
            .await
            .unwrap();

        // Step 3: Client tries to renew old IP and gets NAKed
        let mut renew_request = Message::default();
        renew_request.set_opcode(Opcode::BootRequest);
        renew_request.set_xid(0x12345678);
        renew_request.set_chaddr(&[0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff]);
        renew_request.set_ciaddr(old_ip);
        renew_request
            .opts_mut()
            .insert(v4::DhcpOption::MessageType(MessageType::Request));

        let network = store::get_network(&conn, network_id).await.unwrap();
        let response = handler
            .handle_request(&conn, &renew_request, &network, handler.server_identifier)
            .await
            .unwrap();

        assert!(response.is_some());
        let msg = response.unwrap();
        let msg_type = msg
            .opts()
            .msg_type()
            .expect("Message type should be present");
        assert_eq!(msg_type, MessageType::Nak, "Should NAK renewal of old IP");

        // Step 4: Client rediscovers and requests the reserved IP
        let mut new_request = Message::default();
        new_request.set_opcode(Opcode::BootRequest);
        new_request.set_xid(0x87654321);
        new_request.set_chaddr(&[0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff]);
        new_request
            .opts_mut()
            .insert(v4::DhcpOption::MessageType(MessageType::Request));
        new_request
            .opts_mut()
            .insert(v4::DhcpOption::RequestedIpAddress(reserved_ip));

        let response = handler
            .handle_request(&conn, &new_request, &network, handler.server_identifier)
            .await
            .unwrap();

        // Should receive ACK with reserved IP
        assert!(response.is_some());
        let ack = response.unwrap();
        let msg_type = ack
            .opts()
            .msg_type()
            .expect("Message type should be present");
        assert_eq!(
            msg_type,
            MessageType::Ack,
            "Should ACK request for reserved IP"
        );
        assert_eq!(
            ack.yiaddr(),
            reserved_ip,
            "ACK should contain the reserved IP"
        );

        // Verify lease was updated to new IP
        let lease = store::get_lease_by_mac(&conn, mac).await.unwrap();
        assert!(lease.is_some());
        let lease = lease.unwrap();
        assert_eq!(
            lease.ip_address,
            reserved_ip.to_string(),
            "Lease should be updated to reserved IP"
        );
        assert_eq!(lease.state, LeaseState::Active, "Lease should be active");
    }
}
