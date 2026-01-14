mod allocator;
mod boot_config;
mod handler;
mod ip_discovery;
mod store;

use anyhow::Result;
use rusqlite::Connection;
use std::net::Ipv4Addr;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::{net::UdpSocket, task::JoinHandle};

use crate::director::Director;

pub use ip_discovery::discover_server_identifier;
pub use store::{DhcpNetwork, DhcpPool, DhcpStore, Lease, LeaseState, StaticReservation};

use allocator::IpAllocator;
use boot_config::BootConfigProvider;
use handler::DhcpHandler;

pub struct DhcpServer {
    handler: DhcpHandler,
    address: String,
}

pub struct StartResult {
    pub join_handle: JoinHandle<Result<()>>,
    pub port: u16,
}

impl DhcpServer {
    pub async fn new(
        db: Arc<Mutex<Connection>>,
        director: Director,
        tftp_server: String,
        http_server: String,
        server_identifier: Ipv4Addr,
        enable_autodiscover: bool,
        address: Option<String>,
    ) -> Result<Self> {
        let store = DhcpStore::new(db);

        log::info!("DHCP server configuration:");
        log::info!("  TFTP Server: {}", tftp_server);
        log::info!("  HTTP Server: {}", http_server);
        log::info!("  Server Identifier: {}", server_identifier);
        log::info!(
            "  Autodiscover: {}",
            if enable_autodiscover {
                "enabled"
            } else {
                "disabled"
            }
        );

        // List networks at startup for diagnostic purposes
        let networks = store.list_networks().await?;
        log::info!("  Configured networks: {}", networks.len());
        for network in &networks {
            log::info!(
                "    - {} (id={}, relay={:?})",
                network.name,
                network.id,
                network.relay_agent_address
            );
        }

        let allocator = IpAllocator::new(store.clone());
        let boot_config = BootConfigProvider::new(tftp_server, http_server, enable_autodiscover);
        let handler = DhcpHandler::new(store, director, allocator, boot_config, server_identifier);

        Ok(Self {
            handler,
            address: address.unwrap_or("0.0.0.0:67".to_string()),
        })
    }

    /// Start the DHCP server (long-running task)
    pub async fn serve(self) -> Result<StartResult> {
        let socket = Arc::new(UdpSocket::bind(&self.address).await?);
        let local_addr = socket.local_addr()?;
        log::info!("DHCP server listening on {}", local_addr);

        let join_handle = tokio::spawn(self.serve_task(socket));
        let port = local_addr.port();
        Ok(StartResult { join_handle, port })
    }

    pub async fn serve_task(self, socket: Arc<UdpSocket>) -> Result<()> {
        let mut buf = vec![0u8; 1500]; // MTU size

        loop {
            match socket.recv_from(&mut buf).await {
                Ok((len, peer_addr)) => {
                    let data = buf[..len].to_vec();
                    let handler = self.handler.clone();
                    let socket_clone = socket.clone();

                    // Spawn task per request (like TFTP server pattern)
                    tokio::spawn(async move {
                        if let Err(e) = handler.handle_packet(&data, peer_addr, socket_clone).await
                        {
                            log::error!("DHCP handler error: {}", e);
                        }
                    });
                }
                Err(e) => {
                    log::error!("DHCP socket error: {}", e);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::storage::MemoryImageStore;

    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_dhcp_server_creation() {
        let temp_dir = tempdir().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let conn = crate::database::open(db_path).unwrap();
        let db = Arc::new(Mutex::new(conn));
        let director = Director::new(
            db.clone(),
            Arc::new(MemoryImageStore::new()),
            "http://localhost:8080",
        );

        let server_identifier = "10.0.0.1".parse().unwrap();
        let server = DhcpServer::new(
            db,
            director,
            "10.0.0.1:69".to_string(),
            "http://10.0.0.1:3000".to_string(),
            server_identifier,
            false,
            Some("0.0.0.0:6767".to_string()),
        )
        .await
        .unwrap();
        assert_eq!(server.address, "0.0.0.0:6767".to_string());
    }
}
