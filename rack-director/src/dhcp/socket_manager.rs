use std::collections::HashMap;
use std::net::Ipv4Addr;
use std::sync::Arc;

use tokio::net::UdpSocket;
use tokio::sync::{mpsc, oneshot, watch};
use tokio::task::AbortHandle;
use tokio_recvmsg::UdpSocketRecvMsg;

use super::handler::{DhcpHandler, DhcpReply};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// The read-hot socket table, shared via `watch::Receiver`.
///
/// All DHCP receive loops borrow this table when dispatching replies so they
/// always use the most-recently-bound socket for each network.
pub struct SocketTable {
    /// The socket bound to `server_identifier:port` — used for relay-agent
    /// replies and as the send socket for networks whose local IP equals the
    /// server identifier.
    pub server_id_socket: Arc<UdpSocket>,
    /// Per-network sockets keyed by the local interface IP for that network.
    pub network_sockets: HashMap<Ipv4Addr, Arc<UdpSocket>>,
}

/// Commands sent from the HTTP layer to the `DhcpSocketManager`.
pub enum SocketCmd {
    /// A new DHCP network was created via the HTTP API. The manager should
    /// bind a socket for the network's local interface IP (if one exists).
    AddNetwork {
        id: i64,
        subnet: String,
        resp: oneshot::Sender<()>,
    },
    /// A DHCP network was deleted via the HTTP API. The manager should close
    /// the associated socket (if any).
    RemoveNetwork { id: i64, resp: oneshot::Sender<()> },
}

// ---------------------------------------------------------------------------
// Private implementation types
// ---------------------------------------------------------------------------

/// Bookkeeping for a single managed network socket entry.
struct NetworkEntry {
    /// The local interface IP that was bound for this network.
    local_ip: Ipv4Addr,
    /// The bound socket (may alias `server_id_socket` when
    /// `local_ip == server_identifier`).
    socket: Arc<UdpSocket>,
    /// Abort handle for the per-network receive loop. `None` when this
    /// entry reuses the server-id receive loop (i.e. `local_ip == server_identifier`).
    recv_abort: Option<AbortHandle>,
}

/// Owns all DHCP socket lifecycle and reacts to `SocketCmd` events.
///
/// The manager is intentionally free of locks. Callers communicate via
/// `mpsc` commands; readers get the current socket table via `watch`.
pub struct DhcpSocketManager {
    port: u16,
    server_identifier: Ipv4Addr,
    server_id_socket: Arc<UdpSocket>,
    /// Abort handle kept so callers can cancel the server-id loop on shutdown.
    server_id_recv: AbortHandle,
    network_entries: HashMap<i64, NetworkEntry>,
    table_tx: watch::Sender<Arc<SocketTable>>,
    cmd_rx: mpsc::Receiver<SocketCmd>,
    handler: DhcpHandler,
}

impl DhcpSocketManager {
    /// Create a new manager from pre-built components.
    ///
    /// Callers must:
    /// 1. Bind `server_id_socket` and enable pktinfo on it.
    /// 2. Spawn the server-id receive loop and pass its `AbortHandle`.
    /// 3. Build an initial (empty) `SocketTable` and create a `watch` pair.
    /// 4. Create an `mpsc` pair and pass `cmd_rx` here, keeping `cmd_tx` for
    ///    `DhcpControl`.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        port: u16,
        server_identifier: Ipv4Addr,
        server_id_socket: Arc<UdpSocket>,
        server_id_recv: AbortHandle,
        table_tx: watch::Sender<Arc<SocketTable>>,
        cmd_rx: mpsc::Receiver<SocketCmd>,
        handler: DhcpHandler,
    ) -> Self {
        Self {
            port,
            server_identifier,
            server_id_socket,
            server_id_recv,
            network_entries: HashMap::new(),
            table_tx,
            cmd_rx,
            handler,
        }
    }

    /// Run the command loop. Returns when the command channel is closed.
    pub async fn run(mut self) {
        while let Some(cmd) = self.cmd_rx.recv().await {
            match cmd {
                SocketCmd::AddNetwork { id, subnet, resp } => {
                    self.handle_add_network(id, subnet);
                    let _ = resp.send(());
                }
                SocketCmd::RemoveNetwork { id, resp } => {
                    self.handle_remove_network(id);
                    let _ = resp.send(());
                }
            }
        }
        // Channel closed — abort server-id recv loop on the way out.
        self.server_id_recv.abort();
    }

    // -----------------------------------------------------------------------
    // Command handlers
    // -----------------------------------------------------------------------

    /// Handle `AddNetwork`: resolve local IP, bind socket (or reuse server-id
    /// socket), spawn per-network recv loop if needed, rebuild table.
    fn handle_add_network(&mut self, id: i64, subnet: String) {
        let local_ip = match super::interface::find_local_ip_for_subnet(&subnet) {
            Ok(Some(ip)) => ip,
            Ok(None) => {
                log::warn!(
                    "DHCP socket manager: no local interface found for subnet {} (network id={}), skipping",
                    subnet,
                    id
                );
                return;
            }
            Err(e) => {
                log::warn!(
                    "DHCP socket manager: failed to resolve subnet {} (network id={}): {}",
                    subnet,
                    id,
                    e
                );
                return;
            }
        };

        let entry = if local_ip == self.server_identifier {
            // The network's local IP is the same as the server identifier —
            // the server-id socket already handles packets on this IP.
            log::info!(
                "DHCP socket manager: network {} ({}) reuses server-id socket ({}:{})",
                id,
                subnet,
                local_ip,
                self.port
            );
            NetworkEntry {
                local_ip,
                socket: self.server_id_socket.clone(),
                recv_abort: None,
            }
        } else {
            // Bind a dedicated socket for this network.
            let socket = match bind_l2_socket(local_ip, self.port) {
                Ok(s) => Arc::new(s),
                Err(e) => {
                    log::warn!(
                        "DHCP socket manager: failed to bind socket for network {} ({}:{}): {}",
                        id,
                        local_ip,
                        self.port,
                        e
                    );
                    return;
                }
            };

            log::info!(
                "DHCP socket manager: bound socket {}:{} for network {} ({})",
                local_ip,
                self.port,
                id,
                subnet
            );

            let abort = spawn_per_network_recv_loop(
                socket.clone(),
                local_ip,
                self.handler.clone(),
                self.table_tx.subscribe(),
            );

            NetworkEntry {
                local_ip,
                socket,
                recv_abort: Some(abort),
            }
        };

        self.network_entries.insert(id, entry);
        self.publish_table();
    }

    /// Handle `RemoveNetwork`: remove entry, abort recv loop if dedicated,
    /// rebuild table.
    fn handle_remove_network(&mut self, id: i64) {
        if let Some(entry) = self.network_entries.remove(&id) {
            if let Some(abort) = entry.recv_abort {
                abort.abort();
            }
            log::info!(
                "DHCP socket manager: removed socket for network {} ({}:{})",
                id,
                entry.local_ip,
                self.port
            );
            self.publish_table();
        }
    }

    // -----------------------------------------------------------------------
    // Table management
    // -----------------------------------------------------------------------

    /// Rebuild `SocketTable` from current entries and publish it.
    fn publish_table(&self) {
        let table = Arc::new(SocketTable {
            server_id_socket: self.server_id_socket.clone(),
            network_sockets: self
                .network_entries
                .values()
                .map(|e| (e.local_ip, e.socket.clone()))
                .collect(),
        });
        // `send` only fails if all receivers have been dropped (i.e. shutdown),
        // which is benign.
        let _ = self.table_tx.send(table);
    }
}

// ---------------------------------------------------------------------------
// Receive loops (pub so mod.rs can spawn them)
// ---------------------------------------------------------------------------

/// Receive loop for the server-identifier socket. Handles both relay-agent
/// unicast (giaddr-directed) and direct unicast from clients.
///
/// Uses `recvmsg` so that pktinfo identifies the local destination IP, which
/// the handler uses for network-lookup and `server_identifier` insertion.
pub async fn server_id_recv_loop(
    socket: Arc<UdpSocket>,
    handler: DhcpHandler,
    table_rx: watch::Receiver<Arc<SocketTable>>,
) {
    pktinfo_recv_loop(socket, "server-id socket", handler, table_rx).await
}

/// Receive loop for the wildcard 0.0.0.0 socket (broadcast / relay-agent
/// packets). Only started when `--no-dhcp-broadcast` is **not** set.
pub async fn wildcard_recv_loop(
    socket: UdpSocket,
    handler: DhcpHandler,
    table_rx: watch::Receiver<Arc<SocketTable>>,
) {
    pktinfo_recv_loop(Arc::new(socket), "wildcard socket", handler, table_rx).await
}

/// Inner recv loop used by both `server_id_recv_loop` and `wildcard_recv_loop`.
async fn pktinfo_recv_loop(
    socket: Arc<UdpSocket>,
    label: &'static str,
    handler: DhcpHandler,
    table_rx: watch::Receiver<Arc<SocketTable>>,
) {
    let mut buf = vec![0u8; 1500];
    loop {
        match socket.recv_msg(&mut buf).await {
            Ok((len, pkt_info)) => {
                let data = buf[..len].to_vec();
                let h = handler.clone();
                let rx = table_rx.clone();
                tokio::spawn(async move {
                    match h.handle_packet(&data, &pkt_info).await {
                        Ok(Some(reply)) => dispatch_reply(reply, &rx).await,
                        Ok(None) => {}
                        Err(e) => log::error!("DHCP {} handler error: {}", label, e),
                    }
                });
            }
            Err(e) => log::error!("DHCP {} recv error: {}", label, e),
        }
    }
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

/// Receive loop for a socket bound to a specific local interface IP. Handles
/// unicast DHCP renewals from clients that already have a lease.
///
/// Uses plain `recv_from` (no pktinfo needed) because the local IP is already
/// known from the socket's bound address.
async fn per_network_recv_loop(
    socket: Arc<UdpSocket>,
    local_ip: Ipv4Addr,
    handler: DhcpHandler,
    table_rx: watch::Receiver<Arc<SocketTable>>,
) {
    let mut buf = vec![0u8; 1500];
    loop {
        match socket.recv_from(&mut buf).await {
            Ok((len, peer_addr)) => {
                let data = buf[..len].to_vec();
                let h = handler.clone();
                let rx = table_rx.clone();
                tokio::spawn(async move {
                    match h.handle_l2_unicast_packet(&data, peer_addr, local_ip).await {
                        Ok(Some(reply)) => dispatch_reply(reply, &rx).await,
                        Ok(None) => {}
                        Err(e) => log::error!("DHCP per-network socket handler error: {}", e),
                    }
                });
            }
            Err(e) => log::error!("DHCP per-network socket recv error: {}", e),
        }
    }
}

/// Send a DHCP reply using the appropriate socket from the current table.
///
/// For `L2` replies the socket bound to `local_ip` is used so replies egress
/// on the correct interface and carry the server's source port (required by
/// `dhclient`'s BPF filter: `udp src port 67 and dst port 68`).
///
/// For `Relay` replies the server-id socket is used.
async fn dispatch_reply(reply: DhcpReply, table_rx: &watch::Receiver<Arc<SocketTable>>) {
    use std::net::{IpAddr, SocketAddr};

    match reply {
        DhcpReply::L2 {
            data,
            local_ip,
            peer_addr,
        } => {
            // RFC 2131: broadcast when the client has no IP yet (INIT/SELECTING),
            // unicast when the client already has an IP (RENEWING/REBINDING).
            let dest: SocketAddr = if peer_addr.ip() == IpAddr::V4(Ipv4Addr::UNSPECIFIED) {
                SocketAddr::new(Ipv4Addr::BROADCAST.into(), 68)
            } else {
                peer_addr
            };

            let socket = {
                let table = table_rx.borrow();
                table.network_sockets.get(&local_ip).cloned()
            };

            if let Some(sock) = socket {
                if let Err(e) = sock.send_to(&data, dest).await {
                    log::error!("DHCP L2 send error to {}: {}", dest, e);
                }
            } else {
                log::error!(
                    "DHCP dispatch: no L2 socket for {} — dropping reply to {}",
                    local_ip,
                    dest
                );
            }
        }
        DhcpReply::Relay { data, dest } => {
            let socket = {
                let table = table_rx.borrow();
                table.server_id_socket.clone()
            };
            if let Err(e) = socket.send_to(&data, dest).await {
                log::error!("DHCP relay send error to {}: {}", dest, e);
            }
        }
    }
}

/// Spawn `per_network_recv_loop` and return an `AbortHandle` so the caller
/// can cancel the loop when the network is removed.
fn spawn_per_network_recv_loop(
    socket: Arc<UdpSocket>,
    local_ip: Ipv4Addr,
    handler: DhcpHandler,
    table_rx: watch::Receiver<Arc<SocketTable>>,
) -> AbortHandle {
    tokio::spawn(per_network_recv_loop(socket, local_ip, handler, table_rx)).abort_handle()
}

/// Bind a UDP socket to `local_ip:port` using SO_REUSEADDR and SO_BROADCAST.
///
/// This is a synchronous wrapper around `socket2` so it can be called from
/// within `handle_add_network` without async friction.
fn bind_l2_socket(local_ip: Ipv4Addr, port: u16) -> anyhow::Result<UdpSocket> {
    use socket2::{Domain, Protocol, Socket, Type};
    use std::net::SocketAddrV4;

    let sock = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    sock.set_reuse_address(true)?;
    sock.set_broadcast(true)?;
    sock.bind(&SocketAddrV4::new(local_ip, port).into())?;
    sock.set_nonblocking(true)?;
    Ok(UdpSocket::from_std(sock.into())?)
}
