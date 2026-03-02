mod allocator;
mod boot_config;
mod device_resolution;
mod handler;
mod interface;
mod ip_discovery;
pub mod message_builder;
mod request;
pub mod store;

use anyhow::Result;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::Arc;
use tokio::{net::UdpSocket, task::JoinHandle};
use tokio_recvmsg::UdpSocketRecvMsg;

use crate::database::ConnectionFactory;

pub use ip_discovery::discover_server_identifier;
pub use store::{DhcpNetwork, DhcpPool, Lease, LeaseState, StaticReservation};

use boot_config::BootConfigProvider;
use device_resolution::DirectorDeviceResolver;
use handler::{DhcpHandler, DhcpReply};

use crate::boot_files::BootFileProvider;

/// A UDP socket bound to a specific local IP for one L2 network.
struct L2Socket {
    local_ip: Ipv4Addr,
    socket: Arc<UdpSocket>,
}

pub struct DhcpServer {
    handler: DhcpHandler,
    address: SocketAddr,
    conn: Arc<dyn ConnectionFactory>,
}

pub struct StartResult {
    pub join_handle: JoinHandle<Result<()>>,
    pub port: u16,
}

impl DhcpServer {
    pub async fn new(
        conn: Arc<dyn ConnectionFactory>,
        tftp_server: String,
        http_server: String,
        boot_file_provider: Arc<dyn BootFileProvider>,
        server_identifier: Ipv4Addr,
        address: Option<SocketAddr>,
    ) -> Result<Self> {
        log::info!("DHCP server configuration:");
        log::info!("  TFTP Server: {}", tftp_server);
        log::info!("  HTTP Server: {}", http_server);
        log::info!("  Server Identifier: {}", server_identifier);

        let startup_db = conn.open().await?;
        let networks = store::list_networks(&startup_db).await?;
        log::info!("  Configured networks: {}", networks.len());
        for network in &networks {
            log::info!(
                "    - {} (id={}, relay={:?}, autodiscover={})",
                network.name,
                network.id,
                network.relay_agent_address,
                network.enable_autodiscovery
            );
        }

        let boot_config = BootConfigProvider::new(tftp_server, http_server, boot_file_provider);
        let device_resolver = Arc::new(DirectorDeviceResolver::new());
        let handler = DhcpHandler::new(
            conn.clone(),
            device_resolver,
            boot_config,
            server_identifier,
        );

        Ok(Self {
            handler,
            address: address.unwrap_or_else(|| SocketAddr::new(server_identifier.into(), 67)),
            conn,
        })
    }

    /// Start the DHCP server (long-running task).
    pub async fn serve(self) -> Result<StartResult> {
        // Bind wildcard socket first to obtain the ephemeral port, then bind
        // per-network sockets to the SAME port on specific IPs. All sockets use
        // SO_REUSEADDR so they can coexist on the same port.
        let wildcard = make_wildcard_socket(&self.address).await?;
        wildcard.set_broadcast(true)?;
        wildcard.enable_pktinfo()?;

        let local_addr = wildcard.local_addr()?;
        log::info!("DHCP server listening on {}", local_addr);
        let port = local_addr.port();

        let startup_conn = self.conn.open().await?;
        let l2_sockets: Arc<Vec<L2Socket>> = Arc::new(build_l2_sockets(&startup_conn, port).await?);
        drop(startup_conn);

        // Spawn per-network unicast receive loops.
        for l2 in l2_sockets.iter() {
            let sock = l2.socket.clone();
            let local_ip = l2.local_ip;
            let handler = self.handler.clone();
            let l2_ref = l2_sockets.clone();
            tokio::spawn(async move {
                per_network_recv_loop(sock, local_ip, handler, l2_ref).await;
            });
        }

        // Spawn wildcard broadcast receive loop
        let join_handle = tokio::spawn(wildcard_recv_loop(wildcard, self.handler, l2_sockets));

        Ok(StartResult { join_handle, port })
    }
}

/// Build the wildcard 0.0.0.0 socket with SO_REUSEADDR so per-network sockets can
/// coexist on the same port. tokio's `UdpSocket::bind` does not expose SO_REUSEADDR
/// before binding, so socket2 is used directly.
async fn make_wildcard_socket(address: &SocketAddr) -> anyhow::Result<UdpSocket> {
    use socket2::{Domain, Protocol, Socket, Type};
    let sock = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    sock.set_reuse_address(true)?;
    sock.set_broadcast(true)?;
    sock.bind(&(*address).into())?;
    sock.set_nonblocking(true)?;
    Ok(UdpSocket::from_std(sock.into())?)
}

/// Build a UDP socket bound to `local_ip:port` with SO_REUSEADDR and SO_BROADCAST.
///
/// The port must match the wildcard socket's port so all sockets share the same
/// source port (required by dhclient's BPF filter: `udp src port 67 and dst port 68`).
async fn make_l2_socket(local_ip: Ipv4Addr, port: u16) -> anyhow::Result<UdpSocket> {
    use socket2::{Domain, Protocol, Socket, Type};
    let sock = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    sock.set_reuse_address(true)?;
    sock.set_broadcast(true)?;
    sock.bind(&SocketAddrV4::new(local_ip, port).into())?;
    sock.set_nonblocking(true)?;
    Ok(UdpSocket::from_std(sock.into())?)
}

/// Enumerate all local interfaces and create one L2Socket per L2 network that has
/// a matching local interface IP. All sockets bind to `local_ip:port` where `port`
/// matches the wildcard socket so replies egress with the correct source port.
async fn build_l2_sockets(
    conn: &crate::database::Connection,
    port: u16,
) -> anyhow::Result<Vec<L2Socket>> {
    use network_interface::{Addr, NetworkInterface, NetworkInterfaceConfig};

    let networks = store::get_l2_networks(conn).await?;
    let all_ifaces = NetworkInterface::show()?;
    let mut sockets = Vec::new();

    for network in &networks {
        let subnet: common::Ipv4Subnet = network
            .subnet
            .parse()
            .map_err(|e: common::Ipv4SubnetError| anyhow::anyhow!("{}", e))?;

        let local_ip = all_ifaces
            .iter()
            .flat_map(|iface| &iface.addr)
            .filter_map(|addr| match addr {
                Addr::V4(v4) => Some(v4.ip),
                _ => None,
            })
            .find(|&ip| subnet.ip_in_range(ip));

        let Some(local_ip) = local_ip else {
            log::warn!(
                "No local interface found for L2 network '{}' ({}), skipping socket",
                network.name,
                network.subnet
            );
            continue;
        };

        match make_l2_socket(local_ip, port).await {
            Ok(sock) => {
                log::info!(
                    "L2 socket bound to {}:{} for network '{}'",
                    local_ip,
                    port,
                    network.name
                );
                sockets.push(L2Socket {
                    local_ip,
                    socket: Arc::new(sock),
                });
            }
            Err(e) => {
                log::warn!(
                    "Failed to bind L2 socket for network '{}' ({}): {}",
                    network.name,
                    local_ip,
                    e
                );
            }
        }
    }

    Ok(sockets)
}

/// Receive loop for the wildcard 0.0.0.0:67 socket. Uses recvmsg to capture
/// the incoming interface index so we can select the correct L2 network.
async fn wildcard_recv_loop(
    socket: UdpSocket,
    handler: DhcpHandler,
    l2_sockets: Arc<Vec<L2Socket>>,
) -> Result<()> {
    let mut buf = vec![0u8; 1500];
    loop {
        match socket.recv_msg(&mut buf).await {
            Ok((len, pkt_info)) => {
                let data = buf[..len].to_vec();
                let h = handler.clone();
                let l2 = l2_sockets.clone();
                tokio::spawn(async move {
                    match h.handle_packet(&data, &pkt_info).await {
                        Ok(Some(reply)) => dispatch_reply(reply, &l2).await,
                        Ok(None) => {}
                        Err(e) => log::error!("DHCP handler error: {}", e),
                    }
                });
            }
            Err(e) => log::error!("DHCP wildcard socket error: {}", e),
        }
    }
}

/// Receive loop for a per-network socket bound to `local_ip:67`. Handles unicast
/// DHCP renewals and rebinding from clients that already have a lease.
async fn per_network_recv_loop(
    socket: Arc<UdpSocket>,
    local_ip: Ipv4Addr,
    handler: DhcpHandler,
    l2_sockets: Arc<Vec<L2Socket>>,
) {
    let mut buf = vec![0u8; 1500];
    loop {
        match socket.recv_from(&mut buf).await {
            Ok((len, peer_addr)) => {
                let data = buf[..len].to_vec();
                let h = handler.clone();
                let l2 = l2_sockets.clone();
                tokio::spawn(async move {
                    match h.handle_l2_unicast_packet(&data, peer_addr, local_ip).await {
                        Ok(Some(reply)) => dispatch_reply(reply, &l2).await,
                        Ok(None) => {}
                        Err(e) => log::error!("DHCP unicast handler error: {}", e),
                    }
                });
            }
            Err(e) => log::error!("DHCP per-network socket error: {}", e),
        }
    }
}

/// Send a DHCP reply using the appropriate socket.
///
/// For L2 replies: use the per-network socket bound to `local_ip:port` so that
/// replies egress on the correct interface and carry the same source port as the
/// server (required by `dhclient`'s BPF filter).
async fn dispatch_reply(reply: DhcpReply, l2_sockets: &[L2Socket]) {
    match reply {
        DhcpReply::L2 {
            data,
            local_ip,
            peer_addr,
        } => {
            // RFC 2131: if the client has no IP yet (INIT/SELECTING), broadcast the reply.
            // Otherwise (RENEWING/REBINDING), reply directly to peer_addr.
            let dest: SocketAddr = if peer_addr.ip().is_unspecified() {
                SocketAddr::new(Ipv4Addr::BROADCAST.into(), 68)
            } else {
                peer_addr
            };

            if let Some(l2) = l2_sockets.iter().find(|s| s.local_ip == local_ip) {
                if let Err(e) = l2.socket.send_to(&data, dest).await {
                    log::error!("DHCP L2 send error to {}: {}", dest, e);
                }
            } else {
                log::error!("No L2 socket for {} — dropping reply to {}", local_ip, dest);
            }
        }
        DhcpReply::Relay { data, dest } => {
            // Relay reply — source IP doesn't matter, bind ephemerally
            match UdpSocket::bind("0.0.0.0:0").await {
                Ok(s) => {
                    if let Err(e) = s.send_to(&data, dest).await {
                        log::error!("DHCP relay send error: {}", e);
                    }
                }
                Err(e) => log::error!("DHCP relay socket bind error: {}", e),
            }
        }
    }
}

/// Spawn a background task that periodically deletes expired DHCP leases.
pub fn spawn_lease_cleanup_task(connection_factory: Arc<dyn ConnectionFactory>) -> JoinHandle<()> {
    tokio::spawn(async move {
        let conn = connection_factory
            .open()
            .await
            .expect("Failed to open database");
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
        loop {
            interval.tick().await;
            match store::delete_expired_leases(&conn).await {
                Ok(count) if count > 0 => log::info!("Cleaned up {} expired DHCP lease(s)", count),
                Ok(_) => {}
                Err(e) => log::error!("Failed to clean up expired DHCP leases: {}", e),
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_dhcp_server_creation() {
        use crate::boot_files::FilesystemBootFileProvider;
        use tempfile::tempdir;

        let conn: Arc<dyn ConnectionFactory> = Arc::new(crate::test_connection_factory!());
        // Run migrations and keep the connection alive so the in-memory DB persists
        let _migration_conn = crate::database::run_migrations(conn.as_ref())
            .await
            .unwrap();

        // Create a temporary boot files directory for testing
        let temp_dir = tempdir().unwrap();
        let boot_files_dir = temp_dir.path().join("boot_files");
        std::fs::create_dir_all(&boot_files_dir).unwrap();
        let boot_file_provider =
            Arc::new(FilesystemBootFileProvider::new(boot_files_dir.to_path_buf()).unwrap());

        let server_identifier = "10.0.0.1".parse().unwrap();
        let server = DhcpServer::new(
            conn,
            "10.0.0.1:69".to_string(),
            "http://10.0.0.1:3000".to_string(),
            boot_file_provider,
            server_identifier,
            Some(SocketAddr::new(Ipv4Addr::new(0, 0, 0, 0).into(), 67)),
        )
        .await
        .unwrap();
        assert_eq!(
            server.address,
            SocketAddr::new(Ipv4Addr::new(0, 0, 0, 0).into(), 67)
        );
    }
}
