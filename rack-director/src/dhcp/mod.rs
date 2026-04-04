mod allocator;
mod boot_config;
mod device_resolution;
mod handler;
mod interface;
mod ip_discovery;
pub mod message_builder;
mod request;
pub mod socket_manager;
pub mod store;

use anyhow::Result;
use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::Arc;
use tokio::{net::UdpSocket, sync::mpsc, sync::oneshot, sync::watch, task::JoinHandle};
use tokio_recvmsg::UdpSocketRecvMsg;

use crate::database::ConnectionFactory;

pub use ip_discovery::discover_server_identifier;
pub use socket_manager::SocketCmd;
#[allow(unused_imports)] // re-exported for `crate::dhcp::LeaseState` usage in other modules
pub use store::{DhcpNetwork, DhcpPool, Lease, LeaseState, StaticReservation};

use boot_config::BootConfigProvider;
use device_resolution::DirectorDeviceResolver;
use handler::DhcpHandler;
use socket_manager::{DhcpSocketManager, SocketTable};

pub struct DhcpServer {
    handler: DhcpHandler,
    address: SocketAddr,
    conn: Arc<dyn ConnectionFactory>,
    server_identifier: Ipv4Addr,
}

pub struct StartResult {
    pub join_handle: JoinHandle<()>,
    pub port: u16,
    pub control: DhcpControl,
}

/// A cheaply-clonable handle for notifying the DHCP socket manager of
/// network lifecycle events. Obtained from `StartResult::control`.
#[derive(Clone)]
pub struct DhcpControl {
    cmd_tx: mpsc::Sender<SocketCmd>,
}

impl DhcpControl {
    /// Create a no-op control handle whose commands are immediately discarded.
    ///
    /// Useful in unit tests that construct `AppState` directly but do not
    /// exercise DHCP socket management.
    #[cfg(test)]
    pub fn noop() -> Self {
        // Capacity 1; the receiver is immediately dropped so the manager never
        // runs. Commands sent via DhcpControl are silently discarded.
        let (cmd_tx, _cmd_rx) = mpsc::channel(1);
        Self { cmd_tx }
    }

    /// Notify the socket manager that a new DHCP network was created.
    ///
    /// Returns once the manager has processed the command (socket bound or
    /// warning logged if no matching interface was found).
    pub async fn network_created(&self, id: i64, subnet: String) {
        let (resp_tx, resp_rx) = oneshot::channel();
        if self
            .cmd_tx
            .send(SocketCmd::AddNetwork {
                id,
                subnet,
                resp: resp_tx,
            })
            .await
            .is_ok()
        {
            resp_rx.await.ok();
        }
    }

    /// Notify the socket manager that a DHCP network was deleted.
    ///
    /// Returns once the manager has processed the command (socket closed if
    /// one was bound for this network).
    pub async fn network_deleted(&self, id: i64) {
        let (resp_tx, resp_rx) = oneshot::channel();
        if self
            .cmd_tx
            .send(SocketCmd::RemoveNetwork { id, resp: resp_tx })
            .await
            .is_ok()
        {
            resp_rx.await.ok();
        }
    }
}

impl DhcpServer {
    pub async fn new(
        conn: Arc<dyn ConnectionFactory>,
        tftp_server: String,
        http_server: String,
        boot_file_provider: Arc<dyn crate::boot_files::BootFileProvider>,
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
            server_identifier,
        })
    }

    /// Start the DHCP server.
    ///
    /// When `no_broadcast` is `true` (used in tests), no wildcard socket is
    /// created. The server-id socket binds to `server_identifier:port` and is
    /// the only receive path. This avoids privilege issues and port conflicts
    /// during testing.
    ///
    /// When `no_broadcast` is `false` (production), a wildcard socket binds
    /// `0.0.0.0:PORT` first to obtain the OS-assigned port, and all subsequent
    /// sockets reuse that same port via `SO_REUSEADDR`.
    pub async fn serve(self, no_broadcast: bool) -> Result<StartResult> {
        let (cmd_tx, cmd_rx) = mpsc::channel(32);

        let (port, table_rx, join_handle) = if no_broadcast {
            start_no_broadcast(
                self.server_identifier,
                self.address.port(),
                cmd_rx,
                self.handler.clone(),
            )
            .await?
        } else {
            start_with_broadcast(
                self.server_identifier,
                &self.address,
                cmd_rx,
                self.handler.clone(),
            )
            .await?
        };

        // Bind sockets for networks that already existed at startup.
        bootstrap_existing_networks(&self.conn, &cmd_tx, &table_rx).await;

        log::info!(
            "DHCP server started on port {} (no_broadcast={})",
            port,
            no_broadcast
        );

        Ok(StartResult {
            join_handle,
            port,
            control: DhcpControl { cmd_tx },
        })
    }
}

// ---------------------------------------------------------------------------
// Start helpers
// ---------------------------------------------------------------------------

/// Start in no-broadcast mode: server-id socket only, no wildcard socket.
///
/// Returns `(port, table_rx, join_handle)` where `join_handle` is the socket
/// manager's run loop.
async fn start_no_broadcast(
    server_identifier: Ipv4Addr,
    requested_port: u16,
    cmd_rx: mpsc::Receiver<SocketCmd>,
    handler: DhcpHandler,
) -> Result<(u16, watch::Receiver<Arc<SocketTable>>, JoinHandle<()>)> {
    // Bind server-id socket; port 0 yields an ephemeral port.
    let server_id_socket =
        Arc::new(make_server_id_socket(server_identifier, requested_port).await?);
    let port = server_id_socket.local_addr()?.port();

    log::info!(
        "DHCP server (no-broadcast) listening on {}:{}",
        server_identifier,
        port
    );

    let initial_table = Arc::new(SocketTable {
        server_id_socket: server_id_socket.clone(),
        network_sockets: HashMap::new(),
    });
    let (table_tx, table_rx) = watch::channel(initial_table);

    // Spawn the server-id receive loop.
    let recv_handle = tokio::spawn(socket_manager::server_id_recv_loop(
        server_id_socket.clone(),
        handler.clone(),
        table_rx.clone(),
    ));
    let recv_abort = recv_handle.abort_handle();

    let manager = DhcpSocketManager::new(
        port,
        server_identifier,
        server_id_socket.clone(),
        recv_abort,
        table_tx.clone(),
        cmd_rx,
        handler,
    );
    let join_handle = tokio::spawn(manager.run());

    Ok((port, table_rx, join_handle))
}

/// Start in broadcast mode: wildcard socket + server-id socket.
///
/// Returns `(port, table_rx, join_handle)` where `join_handle` is the socket
/// manager's run loop.
async fn start_with_broadcast(
    server_identifier: Ipv4Addr,
    address: &SocketAddr,
    cmd_rx: mpsc::Receiver<SocketCmd>,
    handler: DhcpHandler,
) -> Result<(u16, watch::Receiver<Arc<SocketTable>>, JoinHandle<()>)> {
    // Bind the wildcard socket first so the OS assigns the port.
    let wildcard = make_wildcard_socket(address).await?;
    wildcard.enable_pktinfo()?;
    let port = wildcard.local_addr()?.port();

    log::info!("DHCP server listening on 0.0.0.0:{}", port);

    // Bind server-id socket to the same port with SO_REUSEADDR.
    let server_id_socket = Arc::new(make_server_id_socket(server_identifier, port).await?);

    let initial_table = Arc::new(SocketTable {
        server_id_socket: server_id_socket.clone(),
        network_sockets: HashMap::new(),
    });
    let (table_tx, table_rx) = watch::channel(initial_table);

    // Spawn server-id receive loop (detached — managed by socket manager).
    let recv_handle = tokio::spawn(socket_manager::server_id_recv_loop(
        server_id_socket.clone(),
        handler.clone(),
        table_rx.clone(),
    ));
    let recv_abort = recv_handle.abort_handle();

    // Spawn wildcard receive loop (detached — runs for the server's lifetime).
    tokio::spawn(socket_manager::wildcard_recv_loop(
        wildcard,
        handler.clone(),
        table_rx.clone(),
    ));

    let manager = DhcpSocketManager::new(
        port,
        server_identifier,
        server_id_socket.clone(),
        recv_abort,
        table_tx.clone(),
        cmd_rx,
        handler,
    );
    let join_handle = tokio::spawn(manager.run());

    Ok((port, table_rx, join_handle))
}

/// Replay all pre-existing networks through the socket manager so that
/// sockets are bound for networks created before this server started.
///
/// This mirrors the behaviour of the old `build_l2_sockets` function but
/// goes through the same `SocketCmd` path as the live API, ensuring the
/// socket table is always the authoritative source.
async fn bootstrap_existing_networks(
    conn: &Arc<dyn ConnectionFactory>,
    cmd_tx: &mpsc::Sender<SocketCmd>,
    table_rx: &watch::Receiver<Arc<SocketTable>>,
) {
    let startup_conn = match conn.open().await {
        Ok(c) => c,
        Err(e) => {
            log::error!("DHCP bootstrap: failed to open DB connection: {}", e);
            return;
        }
    };

    let networks = match store::get_l2_networks(&startup_conn).await {
        Ok(n) => n,
        Err(e) => {
            log::error!("DHCP bootstrap: failed to list networks: {}", e);
            return;
        }
    };

    for network in networks {
        let (resp_tx, resp_rx) = oneshot::channel();
        if cmd_tx
            .send(SocketCmd::AddNetwork {
                id: network.id,
                subnet: network.subnet.clone(),
                resp: resp_tx,
            })
            .await
            .is_ok()
        {
            resp_rx.await.ok();
        }
    }

    log::debug!(
        "DHCP bootstrap: socket table has {} network socket(s)",
        table_rx.borrow().network_sockets.len()
    );
}

// ---------------------------------------------------------------------------
// Socket constructors
// ---------------------------------------------------------------------------

/// Build a wildcard 0.0.0.0 socket with SO_REUSEADDR and SO_BROADCAST so
/// per-network sockets can coexist on the same port. `tokio`'s `UdpSocket::bind`
/// does not expose SO_REUSEADDR before binding, so `socket2` is used directly.
async fn make_wildcard_socket(address: &SocketAddr) -> anyhow::Result<UdpSocket> {
    use socket2::{Domain, Protocol, Socket, Type};
    let sock = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    sock.set_reuse_address(true)?;
    sock.set_broadcast(true)?;
    sock.bind(&(*address).into())?;
    sock.set_nonblocking(true)?;
    Ok(UdpSocket::from_std(sock.into())?)
}

/// Build a socket bound to `local_ip:port` with SO_REUSEADDR, SO_BROADCAST,
/// and IP_PKTINFO enabled.
///
/// This socket serves as the server-identifier socket. Pktinfo is required
/// because the server-id receive loop calls `handle_packet` which uses the
/// destination IP from pktinfo to identify the DHCP network.
pub(crate) async fn make_server_id_socket(
    local_ip: Ipv4Addr,
    port: u16,
) -> anyhow::Result<UdpSocket> {
    use socket2::{Domain, Protocol, Socket, Type};
    let sock = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    sock.set_reuse_address(true)?;
    sock.set_broadcast(true)?;
    sock.bind(&SocketAddrV4::new(local_ip, port).into())?;
    sock.set_nonblocking(true)?;
    let udp = UdpSocket::from_std(sock.into())?;
    udp.enable_pktinfo()?;
    Ok(udp)
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
