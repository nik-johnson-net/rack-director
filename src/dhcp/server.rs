use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;
use tokio::net::UdpSocket as TokioUdpSocket;
use tokio::sync::Mutex;
use rusqlite::Connection;
use chrono::Utc;

use crate::dhcp::{
    Result, DhcpError, MacAddress, Interface, Subnet,
    packet::{DhcpPacket, DhcpMessageType, DhcpOption},
    pool::IpPool,
    option82::Option82Parser,
};

pub struct DhcpServer {
    db: Arc<Mutex<Connection>>,
    ip_pool: Arc<Mutex<IpPool>>,
    server_ipv4: Ipv4Addr,
    server_ipv6: Option<Ipv6Addr>,
}

impl DhcpServer {
    pub fn new(db: Arc<Mutex<Connection>>, server_ipv4: Ipv4Addr, server_ipv6: Option<Ipv6Addr>) -> Self {
        DhcpServer {
            db,
            ip_pool: Arc::new(Mutex::new(IpPool::new())),
            server_ipv4,
            server_ipv6,
        }
    }
    
    pub async fn start(&self) -> Result<()> {
        // Initialize IP pools from database
        self.initialize_pools().await?;
        
        // Start IPv4 DHCP server
        let ipv4_handle = {
            let server = self.clone();
            tokio::spawn(async move {
                if let Err(e) = server.serve_ipv4().await {
                    log::error!("IPv4 DHCP server error: {}", e);
                }
            })
        };
        
        // Start IPv6 DHCP server if configured
        let ipv6_handle = if self.server_ipv6.is_some() {
            let server = self.clone();
            Some(tokio::spawn(async move {
                if let Err(e) = server.serve_ipv6().await {
                    log::error!("IPv6 DHCP server error: {}", e);
                }
            }))
        } else {
            None
        };
        
        // Wait for servers to complete
        if let Err(e) = ipv4_handle.await {
            log::error!("IPv4 DHCP server task error: {}", e);
        }
        
        if let Some(handle) = ipv6_handle {
            if let Err(e) = handle.await {
                log::error!("IPv6 DHCP server task error: {}", e);
            }
        }
        
        Ok(())
    }
    
    async fn initialize_pools(&self) -> Result<()> {
        // Collect subnets first, then release the database lock
        let subnets = {
            let db = self.db.lock().await;
            let mut stmt = db.prepare("SELECT id, name, network_ipv4, network_ipv6, gateway_ipv4, gateway_ipv6, dns_servers, lease_time FROM subnets")
                .map_err(|e| DhcpError::DatabaseError(e.to_string()))?;
            
            let subnet_iter = stmt.query_map([], |row| {
                Ok(Subnet {
                    id: Some(row.get(0)?),
                    name: row.get(1)?,
                    network_ipv4: row.get::<_, Option<String>>(2)?.and_then(|s| s.parse().ok()),
                    network_ipv6: row.get::<_, Option<String>>(3)?.and_then(|s| s.parse().ok()),
                    gateway_ipv4: row.get::<_, Option<String>>(4)?.and_then(|s| s.parse().ok()),
                    gateway_ipv6: row.get::<_, Option<String>>(5)?.and_then(|s| s.parse().ok()),
                    dns_servers: row.get::<_, Option<String>>(6)?
                        .map(|s| serde_json::from_str(&s).unwrap_or_default())
                        .unwrap_or_default(),
                    lease_time: row.get::<_, Option<u32>>(7)?.unwrap_or(3600),
                })
            }).map_err(|e| DhcpError::DatabaseError(e.to_string()))?;
            
            let mut subnets = Vec::new();
            for subnet_result in subnet_iter {
                subnets.push(subnet_result.map_err(|e| DhcpError::DatabaseError(e.to_string()))?);
            }
            subnets
        };
        
        // Now add subnets to the pool
        let mut pool = self.ip_pool.lock().await;
        for subnet in subnets {
            pool.add_subnet(&subnet)?;
        }
        
        Ok(())
    }
    
    async fn serve_ipv4(&self) -> Result<()> {
        let socket = TokioUdpSocket::bind("0.0.0.0:67").await
            .map_err(|e| DhcpError::NetworkError(e.to_string()))?;
        
        log::info!("DHCP IPv4 server listening on 0.0.0.0:67");
        
        let mut buf = [0u8; 1024];
        loop {
            match socket.recv_from(&mut buf).await {
                Ok((len, addr)) => {
                    let data = &buf[..len];
                    if let Err(e) = self.handle_ipv4_packet(data, addr, &socket).await {
                        log::error!("Error handling IPv4 DHCP packet: {}", e);
                    }
                }
                Err(e) => {
                    log::error!("Error receiving IPv4 DHCP packet: {}", e);
                }
            }
        }
    }
    
    async fn serve_ipv6(&self) -> Result<()> {
        let socket = TokioUdpSocket::bind("[::]:547").await
            .map_err(|e| DhcpError::NetworkError(e.to_string()))?;
        
        log::info!("DHCP IPv6 server listening on [::]:547");
        
        let mut buf = [0u8; 1024];
        loop {
            match socket.recv_from(&mut buf).await {
                Ok((len, addr)) => {
                    let data = &buf[..len];
                    if let Err(e) = self.handle_ipv6_packet(data, addr, &socket).await {
                        log::error!("Error handling IPv6 DHCP packet: {}", e);
                    }
                }
                Err(e) => {
                    log::error!("Error receiving IPv6 DHCP packet: {}", e);
                }
            }
        }
    }
    
    async fn handle_ipv4_packet(&self, data: &[u8], client_addr: SocketAddr, socket: &TokioUdpSocket) -> Result<()> {
        let packet = DhcpPacket::parse(data)
            .map_err(|e| DhcpError::ParseError(e))?;
        
        let message_type = packet.get_message_type()
            .ok_or_else(|| DhcpError::ParseError("No message type in DHCP packet".to_string()))?;
        
        log::debug!("Received DHCP {:?} from {}", message_type, client_addr);
        
        let response = match message_type {
            DhcpMessageType::Discover => self.handle_discover(&packet).await?,
            DhcpMessageType::Request => self.handle_request(&packet).await?,
            DhcpMessageType::Release => {
                self.handle_release(&packet).await?;
                return Ok(());
            }
            DhcpMessageType::Decline => {
                self.handle_decline(&packet).await?;
                return Ok(());
            }
            _ => return Ok(()), // Ignore other message types
        };
        
        if let Some(response_packet) = response {
            let response_data = response_packet.serialize();
            
            // Send response to broadcast address if client doesn't have an IP
            let dest_addr = if packet.ciaddr.is_unspecified() {
                SocketAddr::from(([255, 255, 255, 255], 68))
            } else {
                SocketAddr::from((packet.ciaddr.octets(), 68))
            };
            
            socket.send_to(&response_data, dest_addr).await
                .map_err(|e| DhcpError::NetworkError(e.to_string()))?;
            
            log::debug!("Sent DHCP response to {}", dest_addr);
        }
        
        Ok(())
    }
    
    async fn handle_ipv6_packet(&self, _data: &[u8], _client_addr: SocketAddr, _socket: &TokioUdpSocket) -> Result<()> {
        // TODO: Implement DHCPv6 packet handling
        // DHCPv6 has a different packet format and protocol
        log::debug!("IPv6 DHCP packet received (not implemented yet)");
        Ok(())
    }
    
    async fn handle_discover(&self, packet: &DhcpPacket) -> Result<Option<DhcpPacket>> {
        log::debug!("Handling DHCP DISCOVER for MAC: {}", packet.chaddr.to_string());
        
        // Look up or create interface record
        let interface = self.find_or_create_interface(&packet.chaddr, packet).await?;
        
        // Try to get existing IP or allocate new one
        let offered_ip = if let Some(existing_ip) = interface.ipv4_address {
            existing_ip
        } else {
            // Allocate new IP from pool
            let mut pool = self.ip_pool.lock().await;
            pool.allocate_ipv4(interface.subnet_id)
                .ok_or_else(|| DhcpError::NetworkError("No available IP addresses".to_string()))?
        };
        
        // Create OFFER packet
        let mut offer = DhcpPacket::new();
        offer.op = 2; // BOOTREPLY
        offer.xid = packet.xid;
        offer.yiaddr = offered_ip;
        offer.siaddr = self.server_ipv4;
        offer.chaddr = packet.chaddr.clone();
        
        offer.set_message_type(DhcpMessageType::Offer);
        offer.options.insert(54, DhcpOption::ServerIdentifier(self.server_ipv4));
        
        // Add subnet options
        if let Some(subnet) = self.get_subnet_for_interface(&interface).await? {
            if let Some(gateway) = subnet.gateway_ipv4 {
                offer.options.insert(3, DhcpOption::Router(vec![gateway]));
            }
            
            if let Some(network) = subnet.network_ipv4 {
                offer.options.insert(1, DhcpOption::SubnetMask(network.netmask()));
            }
            
            if !subnet.dns_servers.is_empty() {
                let dns_ipv4: Vec<Ipv4Addr> = subnet.dns_servers.iter()
                    .filter_map(|ip| match ip {
                        IpAddr::V4(ipv4) => Some(*ipv4),
                        _ => None,
                    })
                    .collect();
                
                if !dns_ipv4.is_empty() {
                    offer.options.insert(6, DhcpOption::DnsServers(dns_ipv4));
                }
            }
            
            offer.options.insert(51, DhcpOption::LeaseTime(subnet.lease_time));
        }
        
        Ok(Some(offer))
    }
    
    async fn handle_request(&self, packet: &DhcpPacket) -> Result<Option<DhcpPacket>> {
        log::debug!("Handling DHCP REQUEST for MAC: {}", packet.chaddr.to_string());
        
        // Get requested IP address
        let requested_ip = if let Some(DhcpOption::RequestedIpAddress(ip)) = packet.options.get(&50) {
            *ip
        } else if !packet.ciaddr.is_unspecified() {
            packet.ciaddr
        } else {
            return Ok(None);
        };
        
        // Look up interface
        let interface = self.find_or_create_interface(&packet.chaddr, packet).await?;
        
        // Validate the request
        let pool = self.ip_pool.lock().await;
        if !pool.is_available(IpAddr::V4(requested_ip)) && interface.ipv4_address != Some(requested_ip) {
            // Send NAK
            let mut nak = DhcpPacket::new();
            nak.op = 2; // BOOTREPLY
            nak.xid = packet.xid;
            nak.chaddr = packet.chaddr.clone();
            nak.set_message_type(DhcpMessageType::Nak);
            nak.options.insert(54, DhcpOption::ServerIdentifier(self.server_ipv4));
            
            return Ok(Some(nak));
        }
        drop(pool);
        
        // Create lease
        self.create_lease(&interface, IpAddr::V4(requested_ip)).await?;
        
        // Update interface with IP
        self.update_interface_ip(&interface, Some(requested_ip), None).await?;
        
        // Create ACK packet
        let mut ack = DhcpPacket::new();
        ack.op = 2; // BOOTREPLY
        ack.xid = packet.xid;
        ack.yiaddr = requested_ip;
        ack.siaddr = self.server_ipv4;
        ack.chaddr = packet.chaddr.clone();
        
        ack.set_message_type(DhcpMessageType::Ack);
        ack.options.insert(54, DhcpOption::ServerIdentifier(self.server_ipv4));
        
        // Add subnet options (same as in OFFER)
        if let Some(subnet) = self.get_subnet_for_interface(&interface).await? {
            if let Some(gateway) = subnet.gateway_ipv4 {
                ack.options.insert(3, DhcpOption::Router(vec![gateway]));
            }
            
            if let Some(network) = subnet.network_ipv4 {
                ack.options.insert(1, DhcpOption::SubnetMask(network.netmask()));
            }
            
            if !subnet.dns_servers.is_empty() {
                let dns_ipv4: Vec<Ipv4Addr> = subnet.dns_servers.iter()
                    .filter_map(|ip| match ip {
                        IpAddr::V4(ipv4) => Some(*ipv4),
                        _ => None,
                    })
                    .collect();
                
                if !dns_ipv4.is_empty() {
                    ack.options.insert(6, DhcpOption::DnsServers(dns_ipv4));
                }
            }
            
            ack.options.insert(51, DhcpOption::LeaseTime(subnet.lease_time));
        }
        
        Ok(Some(ack))
    }
    
    async fn handle_release(&self, packet: &DhcpPacket) -> Result<()> {
        log::debug!("Handling DHCP RELEASE for MAC: {}", packet.chaddr.to_string());
        
        // Find interface and release IP
        if let Some(interface) = self.find_interface_by_mac(&packet.chaddr).await? {
            if let Some(ip) = interface.ipv4_address {
                self.release_lease(&interface, IpAddr::V4(ip)).await?;
                self.update_interface_ip(&interface, None, None).await?;
                
                let mut pool = self.ip_pool.lock().await;
                pool.release_ip(IpAddr::V4(ip));
            }
        }
        
        Ok(())
    }
    
    async fn handle_decline(&self, packet: &DhcpPacket) -> Result<()> {
        log::debug!("Handling DHCP DECLINE for MAC: {}", packet.chaddr.to_string());
        
        // Mark IP as unavailable
        if let Some(DhcpOption::RequestedIpAddress(ip)) = packet.options.get(&50) {
            let mut pool = self.ip_pool.lock().await;
            pool.mark_used(IpAddr::V4(*ip));
        }
        
        Ok(())
    }
    
    async fn find_or_create_interface(&self, mac: &MacAddress, packet: &DhcpPacket) -> Result<Interface> {
        if let Some(interface) = self.find_interface_by_mac(mac).await? {
            // Update rack information if available from Option 82
            if let Some(DhcpOption::Option82(opt82)) = packet.options.get(&82) {
                if let Some((rack_id, port_id)) = Option82Parser::parse_rack_info(opt82) {
                    self.update_interface_rack_info(&interface, Some(rack_id), Some(port_id)).await?;
                }
            }
            Ok(interface)
        } else {
            // Create new interface
            self.create_interface(mac, packet).await
        }
    }
    
    async fn find_interface_by_mac(&self, mac: &MacAddress) -> Result<Option<Interface>> {
        let db = self.db.lock().await;
        let mut stmt = db.prepare("SELECT id, device_id, mac_address, ipv4_address, ipv6_address, is_bmc, rack_identifier, rack_port, subnet_id FROM interfaces WHERE mac_address = ?1")
            .map_err(|e| DhcpError::DatabaseError(e.to_string()))?;
        
        let interface_iter = stmt.query_map([mac.to_string()], |row| {
            Ok(Interface {
                id: Some(row.get(0)?),
                device_id: row.get(1)?,
                mac_address: MacAddress::from_string(&row.get::<_, String>(2)?)
                    .map_err(|e| rusqlite::Error::FromSqlConversionFailure(2, rusqlite::types::Type::Text, Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, e))))?,
                ipv4_address: row.get::<_, Option<String>>(3)?.and_then(|s| s.parse().ok()),
                ipv6_address: row.get::<_, Option<String>>(4)?.and_then(|s| s.parse().ok()),
                is_bmc: row.get(5)?,
                rack_identifier: row.get(6)?,
                rack_port: row.get(7)?,
                subnet_id: row.get(8)?,
            })
        }).map_err(|e| DhcpError::DatabaseError(e.to_string()))?;
        
        for interface_result in interface_iter {
            let interface = interface_result.map_err(|e| DhcpError::DatabaseError(e.to_string()))?;
            return Ok(Some(interface));
        }
        
        Ok(None)
    }
    
    async fn create_interface(&self, mac: &MacAddress, packet: &DhcpPacket) -> Result<Interface> {
        let db = self.db.lock().await;
        
        // Create a default device if one doesn't exist
        // In a real implementation, you'd want to look up the device properly
        let device_id = 1; // TODO: Implement proper device lookup/creation
        
        // Extract rack info from Option 82 if available
        let (rack_id, port_id) = if let Some(DhcpOption::Option82(opt82)) = packet.options.get(&82) {
            Option82Parser::parse_rack_info(opt82).unwrap_or((String::new(), String::new()))
        } else {
            (String::new(), String::new())
        };
        
        db.execute(
            "INSERT INTO interfaces (device_id, mac_address, is_bmc, rack_identifier, rack_port) VALUES (?1, ?2, ?3, ?4, ?5)",
            [&device_id.to_string(), &mac.to_string(), &false.to_string(), &rack_id, &port_id]
        ).map_err(|e| DhcpError::DatabaseError(e.to_string()))?;
        
        let interface_id = db.last_insert_rowid();
        
        Ok(Interface {
            id: Some(interface_id as i32),
            device_id,
            mac_address: mac.clone(),
            ipv4_address: None,
            ipv6_address: None,
            is_bmc: false,
            rack_identifier: if rack_id.is_empty() { None } else { Some(rack_id) },
            rack_port: if port_id.is_empty() { None } else { Some(port_id) },
            subnet_id: None,
        })
    }
    
    async fn update_interface_ip(&self, interface: &Interface, ipv4: Option<Ipv4Addr>, ipv6: Option<Ipv6Addr>) -> Result<()> {
        let db = self.db.lock().await;
        let ipv4_str = ipv4.map(|ip| ip.to_string());
        let ipv6_str = ipv6.map(|ip| ip.to_string());
        
        db.execute(
            "UPDATE interfaces SET ipv4_address = ?1, ipv6_address = ?2, updated_at = CURRENT_TIMESTAMP WHERE id = ?3",
            rusqlite::params![ipv4_str, ipv6_str, interface.id.unwrap()]
        ).map_err(|e| DhcpError::DatabaseError(e.to_string()))?;
        
        Ok(())
    }
    
    async fn update_interface_rack_info(&self, interface: &Interface, rack_id: Option<String>, port_id: Option<String>) -> Result<()> {
        let db = self.db.lock().await;
        
        db.execute(
            "UPDATE interfaces SET rack_identifier = ?1, rack_port = ?2, updated_at = CURRENT_TIMESTAMP WHERE id = ?3",
            [&rack_id.unwrap_or_default(), &port_id.unwrap_or_default(), &interface.id.unwrap().to_string()]
        ).map_err(|e| DhcpError::DatabaseError(e.to_string()))?;
        
        Ok(())
    }
    
    async fn get_subnet_for_interface(&self, interface: &Interface) -> Result<Option<Subnet>> {
        let db = self.db.lock().await;
        
        let subnet_id = if let Some(id) = interface.subnet_id {
            id
        } else {
            // TODO: Implement subnet selection logic based on rack location or other criteria
            return Ok(None);
        };
        
        let mut stmt = db.prepare("SELECT id, name, network_ipv4, network_ipv6, gateway_ipv4, gateway_ipv6, dns_servers, lease_time FROM subnets WHERE id = ?1")
            .map_err(|e| DhcpError::DatabaseError(e.to_string()))?;
        
        let subnet_iter = stmt.query_map([subnet_id], |row| {
            Ok(Subnet {
                id: Some(row.get(0)?),
                name: row.get(1)?,
                network_ipv4: row.get::<_, Option<String>>(2)?.and_then(|s| s.parse().ok()),
                network_ipv6: row.get::<_, Option<String>>(3)?.and_then(|s| s.parse().ok()),
                gateway_ipv4: row.get::<_, Option<String>>(4)?.and_then(|s| s.parse().ok()),
                gateway_ipv6: row.get::<_, Option<String>>(5)?.and_then(|s| s.parse().ok()),
                dns_servers: row.get::<_, Option<String>>(6)?
                    .map(|s| serde_json::from_str(&s).unwrap_or_default())
                    .unwrap_or_default(),
                lease_time: row.get::<_, Option<u32>>(7)?.unwrap_or(3600),
            })
        }).map_err(|e| DhcpError::DatabaseError(e.to_string()))?;
        
        for subnet_result in subnet_iter {
            let subnet = subnet_result.map_err(|e| DhcpError::DatabaseError(e.to_string()))?;
            return Ok(Some(subnet));
        }
        
        Ok(None)
    }
    
    async fn create_lease(&self, interface: &Interface, ip: IpAddr) -> Result<()> {
        let db = self.db.lock().await;
        let now = Utc::now();
        let lease_end = now + chrono::Duration::seconds(3600); // Default 1 hour lease
        
        db.execute(
            "INSERT INTO dhcp_leases (interface_id, subnet_id, ip_address, lease_start, lease_end, is_active) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            [
                &interface.id.unwrap().to_string(),
                &interface.subnet_id.unwrap_or(0).to_string(),
                &ip.to_string(),
                &now.to_rfc3339(),
                &lease_end.to_rfc3339(),
                &true.to_string()
            ]
        ).map_err(|e| DhcpError::DatabaseError(e.to_string()))?;
        
        Ok(())
    }
    
    async fn release_lease(&self, interface: &Interface, ip: IpAddr) -> Result<()> {
        let db = self.db.lock().await;
        
        db.execute(
            "UPDATE dhcp_leases SET is_active = FALSE WHERE interface_id = ?1 AND ip_address = ?2",
            [&interface.id.unwrap().to_string(), &ip.to_string()]
        ).map_err(|e| DhcpError::DatabaseError(e.to_string()))?;
        
        Ok(())
    }
}

impl Clone for DhcpServer {
    fn clone(&self) -> Self {
        DhcpServer {
            db: Arc::clone(&self.db),
            ip_pool: Arc::clone(&self.ip_pool),
            server_ipv4: self.server_ipv4,
            server_ipv6: self.server_ipv6,
        }
    }
}