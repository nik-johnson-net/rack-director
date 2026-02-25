mod allocator;
mod boot_config;
mod device_resolution;
mod handler;
mod ip_discovery;
pub mod message_builder;
mod request;
pub mod store;

use anyhow::Result;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use tokio::{net::UdpSocket, task::JoinHandle};

use crate::database::ConnectionFactory;

pub use ip_discovery::discover_server_identifier;
pub use store::{DhcpNetwork, DhcpPool, Lease, LeaseState, StaticReservation};

use boot_config::BootConfigProvider;
use device_resolution::DirectorDeviceResolver;
use handler::DhcpHandler;

use crate::boot_files::BootFileProvider;

pub struct DhcpServer {
    handler: DhcpHandler,
    address: SocketAddr,
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

        // List networks at startup for diagnostic purposes
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
        let handler = DhcpHandler::new(conn, device_resolver, boot_config, server_identifier);

        Ok(Self {
            handler,
            address: address.unwrap_or_else(|| SocketAddr::new(server_identifier.into(), 67)),
        })
    }

    /// Start the DHCP server (long-running task)
    pub async fn serve(self) -> Result<StartResult> {
        let socket = Arc::new(UdpSocket::bind(&self.address).await?);
        log::debug!("Enabling broadcast on DHCP socket");
        socket.set_broadcast(true)?;

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
