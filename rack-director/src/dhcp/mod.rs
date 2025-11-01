mod allocator;
mod boot_config;
mod handler;
mod store;

use anyhow::Result;
use rusqlite::Connection;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::{net::UdpSocket, task::JoinHandle};

use crate::director::Director;

pub use store::{DhcpStore, Lease};

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
        address: Option<String>,
    ) -> Result<Self> {
        let store = DhcpStore::new(db);
        let config = store.load_config().await?;

        log::info!("DHCP configuration loaded:");
        log::info!("  Subnet: {}", config.subnet);
        log::info!("  Range: {} - {}", config.range_start, config.range_end);
        log::info!("  Gateway: {}", config.gateway);
        log::info!("  DNS Servers: {:?}", config.dns_servers);
        log::info!("  Lease Duration: {}s", config.lease_duration);
        log::info!("  TFTP Server: {}", config.tftp_server);
        log::info!("  HTTP Server: {}", config.http_server);

        let allocator = IpAllocator::new(store.clone(), director.clone(), config.clone());
        let boot_config =
            BootConfigProvider::new(config.tftp_server.clone(), config.http_server.clone());
        let handler = DhcpHandler::new(store, director, allocator, boot_config);

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
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_dhcp_server_creation() {
        let temp_dir = tempdir().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let conn = crate::database::open(db_path).unwrap();
        let db = Arc::new(Mutex::new(conn));
        let director = Director::new(db.clone());

        let server = DhcpServer::new(db, director, Some("0.0.0.0:6767".to_string()))
            .await
            .unwrap();
        assert_eq!(server.address, "0.0.0.0:6767".to_string());
    }
}
