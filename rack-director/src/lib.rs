mod boot_files;
mod database;
mod device_warnings;
mod dhcp;
mod director;
mod disk_layout;
mod http;
mod lifecycle;
mod operating_systems;
mod osm;
mod plans;
mod platforms;
mod roles;
mod storage;
mod templates;
mod tftp;

use std::{
    io,
    net::{SocketAddr, SocketAddrV4},
    sync::Arc,
};

use anyhow::anyhow;
use clap::Parser;
use tokio::task::JoinHandle;

use crate::storage::ImageStore;

const DEFAULT_DATABASE_PATH: &str = env!("RACK_DIRECTOR_DATABASE_PATH");
const DEFAULT_AGENT_IMAGES_PATH: &str = env!("RACK_DIRECTOR_AGENT_IMAGES_PATH");
const DEFAULT_FIRMWARE_PATH: &str = env!("RACK_DIRECTOR_FIRMWARE_PATH");
const DEFAULT_LOCAL_IMAGES_PATH: &str = env!("RACK_DIRECTOR_LOCAL_IMAGES_PATH");

#[derive(Parser, Debug)]
pub struct Args {
    // Path to the database file.
    #[arg(long, default_value = DEFAULT_DATABASE_PATH)]
    db_path: String,

    // Path to the directory containing the TFTP files.
    #[arg(long, default_value = DEFAULT_FIRMWARE_PATH)]
    tftp_path: String,

    // DHCP server address (optional, defaults to 67)
    #[arg(long)]
    dhcp_address: Option<SocketAddr>,

    // HTTP server address
    #[arg(long, default_value = "0.0.0.0:3000")]
    http_address: SocketAddr,

    // TFTP server address
    #[arg(long, default_value = "0.0.0.0:69")]
    tftp_address: SocketAddr,

    // TFTP server public address (what DHCP advertises to clients)
    #[arg(long)]
    tftp_public_address: Option<String>,

    // HTTP server public url
    #[arg(long)]
    http_public_url: Option<String>,

    // DHCP Server Identifier (Option 54) - the IP address of this DHCP server
    // If not provided, will be auto-discovered or fall back to gateway
    #[arg(long)]
    dhcp_server_identifier: Option<String>,

    // Storage configuration
    #[arg(
        long,
        env,
        default_value = "local",
        help = "Image storage type: local or s3"
    )]
    storage_type: String,

    #[arg(
        long,
        env,
        default_value = DEFAULT_LOCAL_IMAGES_PATH,
        help = "Local storage path (when storage-type=local)"
    )]
    storage_path: String,

    #[arg(
        long,
        env = "AWS_ENDPOINT_URL",
        help = "S3 endpoint URL (when storage-type=s3)"
    )]
    s3_endpoint: Option<String>,

    #[arg(
        long,
        env = "AWS_BUCKET",
        help = "S3 bucket name (when storage-type=s3)"
    )]
    s3_bucket: Option<String>,

    #[arg(
        long,
        env = "AWS_REGION",
        default_value = "us-east-1",
        help = "S3 region (when storage-type=s3)"
    )]
    s3_region: String,

    #[arg(long, help = "Base URL for serving images over HTTP")]
    storage_base_url: Option<String>,

    // Agent images path (bundled with installation)
    #[arg(
        long,
        default_value = DEFAULT_AGENT_IMAGES_PATH,
        help = "Path to agent image files (vmlinuz, initramfs.img)"
    )]
    agent_images_path: String,

    /// Disable the wildcard broadcast socket.
    ///
    /// When set, no `0.0.0.0:PORT` socket is created. The server-identifier
    /// socket handles all traffic. Intended for integration tests where
    /// broadcast sockets are undesirable.
    #[arg(long, default_value_t = false)]
    no_dhcp_broadcast: bool,

    /// Number of seconds unprovisioned devices sleep before rebooting to retry PXE boot.
    #[arg(long, default_value_t = 600)]
    unprovisioned_sleep_secs: u64,
}

pub struct RackDirectorHandle {
    // Information for the http service
    http_handle: JoinHandle<Result<(), io::Error>>,
    pub http_port: u16,

    // Information for the tftp service
    tftp_handle: JoinHandle<Result<(), anyhow::Error>>,
    pub tftp_port: u16,

    // Information for the dhcp service (socket manager run loop, returns ())
    dhcp_handle: JoinHandle<()>,
    pub dhcp_port: u16,

    // Background task for cleaning up expired DHCP leases
    lease_cleanup_handle: JoinHandle<()>,
}

impl RackDirectorHandle {
    // Wait for the services to stop.
    // TODO: Currently just waiting for any one to abort. Need a proper abort signal / shutdown architecture.
    pub async fn wait(self) {
        // dhcp_handle returns () so it cannot be joined with try_join!; abort it on exit.
        let _ = tokio::try_join!(self.http_handle, self.tftp_handle);
        self.dhcp_handle.abort();
        self.lease_cleanup_handle.abort();
    }
}

pub async fn rack_director_start(args: crate::Args) -> Result<RackDirectorHandle, anyhow::Error> {
    let db_file = std::path::PathBuf::from(format!("{}/db.sqlite", args.db_path));

    // Create one shared factory. The factory holds only a PathBuf and opens a
    // fresh connection on each `.open()` call, so sharing it via Arc::clone
    // across Director, DhcpStore, DhcpServer, and the HTTP layer is safe.
    let factory: Arc<dyn database::ConnectionFactory> =
        Arc::new(database::DatabaseConnectionFactory::new(db_file));
    // Run migrations once. For a file-backed database the connection can be
    // dropped immediately after — the schema persists in the file.
    let _ = database::run_migrations(factory.as_ref()).await?;

    // Determine DHCP Server Identifier (Option 54)
    // Priority: CLI arg > auto-discovered IP > fallback to gateway
    let server_identifier = determine_server_identifier(args.dhcp_server_identifier.as_ref())?;

    // Initialize Director
    let public_url = args
        .http_public_url
        .clone()
        .unwrap_or_else(|| format!("http://{}:{}", server_identifier, args.http_address.port()));

    // Initialize storage (after public_url is known so it can be used as the default base URL)
    let storage_config = build_storage_config(&args, &public_url)?;
    let image_store = ImageStore::new(storage_config)?;

    // Cleanup task uses the shared factory.
    let lease_cleanup_handle = dhcp::spawn_lease_cleanup_task(factory.clone());

    // Determine TFTP public address
    let tftp_public = args.tftp_public_address.unwrap_or_else(|| {
        if args.tftp_address.ip().is_unspecified() {
            let addr = SocketAddrV4::new(server_identifier, args.tftp_address.port());
            addr.to_string()
        } else {
            args.tftp_address.to_string()
        }
    });

    let http_server = public_url.clone();

    // Initialize boot file provider for DHCP (Option 13), HTTP Boot, and TFTP
    let boot_file_provider = Arc::new(boot_files::FilesystemBootFileProvider::new(
        std::path::PathBuf::from(&args.tftp_path),
    )?);

    let dhcp_server: dhcp::DhcpServer = dhcp::DhcpServer::new(
        factory.clone(),
        tftp_public.clone(),
        http_server,
        boot_file_provider.clone(),
        server_identifier,
        args.dhcp_address,
    )
    .await
    .unwrap();

    // Initialize TFTP Server
    let mut tftp_server = tftp::Server::new(boot_file_provider.clone());
    tftp_server.address(args.tftp_address);

    // Start DHCP Service first so the DhcpControl handle is available for HTTP.
    let dhcp_start_result = dhcp_server.serve(args.no_dhcp_broadcast).await?;

    // Start HTTP Service — each HTTP handler opens its own connection via the
    // shared factory, keeping it independent of DHCP and Director connections.
    let http_start_result = http::start(
        factory.clone(),
        image_store.into(),
        args.http_address,
        std::path::PathBuf::from(&args.agent_images_path),
        boot_file_provider,
        dhcp_start_result.control.clone(),
        args.unprovisioned_sleep_secs,
    )
    .await?;

    // Start TFTP Service
    let tftp_start_result = tftp_server.serve().await?;

    Ok(RackDirectorHandle {
        http_handle: http_start_result.join_handle,
        http_port: http_start_result.port,

        tftp_handle: tftp_start_result.join_handle,
        tftp_port: tftp_start_result.port,

        dhcp_handle: dhcp_start_result.join_handle,
        dhcp_port: dhcp_start_result.port,

        lease_cleanup_handle,
    })
}

fn build_storage_config(
    args: &Args,
    public_url: &str,
) -> Result<storage::ImageStoreConfig, anyhow::Error> {
    let base_url = args
        .storage_base_url
        .clone()
        .unwrap_or_else(|| format!("{}/cnc/files", public_url));

    match args.storage_type.as_str() {
        "local" => Ok(storage::ImageStoreConfig::Local {
            path: std::path::PathBuf::from(&args.storage_path),
            base_url,
        }),
        "s3" => {
            let endpoint = args
                .s3_endpoint
                .clone()
                .ok_or_else(|| anyhow::anyhow!("--s3-endpoint required when storage-type=s3"))?;
            let bucket = args
                .s3_bucket
                .clone()
                .ok_or_else(|| anyhow::anyhow!("--s3-bucket required when storage-type=s3"))?;

            // Read credentials from environment variables
            let access_key = std::env::var("S3_ACCESS_KEY")
                .or_else(|_| std::env::var("AWS_ACCESS_KEY_ID"))
                .map_err(|_| anyhow::anyhow!("S3_ACCESS_KEY or AWS_ACCESS_KEY_ID environment variable required when storage-type=s3"))?;
            let secret_key = std::env::var("S3_SECRET_KEY")
                .or_else(|_| std::env::var("AWS_SECRET_ACCESS_KEY"))
                .map_err(|_| anyhow::anyhow!("S3_SECRET_KEY or AWS_SECRET_ACCESS_KEY environment variable required when storage-type=s3"))?;

            Ok(storage::ImageStoreConfig::S3 {
                endpoint,
                bucket,
                region: args.s3_region.clone(),
                access_key,
                secret_key,
                base_url,
            })
        }
        _ => Err(anyhow::anyhow!(
            "Invalid storage-type: {}. Must be 'local' or 's3'",
            args.storage_type
        )),
    }
}

/// Determines the DHCP Server Identifier (Option 54) to use for DHCP responses.
///
/// This function implements a three-tier priority system:
/// 1. **CLI Argument**: If `cli_identifier` is provided and valid, use it
/// 2. **Auto-discovery**: Attempt to automatically discover the server's outbound IP
///
/// # Arguments
///
/// * `cli_identifier` - Optional CLI-provided server identifier string
///
/// # Returns
///
/// Returns the determined IPv4 address to use as the DHCP Server Identifier.
///
/// # Examples
///
/// ```ignore
/// // Use CLI-provided identifier
/// let identifier = determine_server_identifier(Some(&"10.0.0.1".to_string()));
///
/// // Auto-discover when no CLI arg provided
/// let identifier = determine_server_identifier(None);
/// ```
fn determine_server_identifier(
    cli_identifier: Option<&String>,
) -> anyhow::Result<std::net::Ipv4Addr> {
    if let Some(identifier_str) = cli_identifier {
        // Use explicitly provided identifier
        match identifier_str.parse() {
            Ok(ip) => {
                log::info!("Using CLI-provided DHCP Server Identifier: {}", ip);
                Ok(ip)
            }
            Err(e) => {
                log::error!(
                    "Invalid DHCP Server Identifier '{}': {}.",
                    identifier_str,
                    e
                );
                Err(anyhow::anyhow!("failed to parse `{identifier_str}`: {e}"))
            }
        }
    } else {
        // No CLI arg provided, attempt auto-discovery
        match dhcp::discover_server_identifier() {
            Ok(ip) => {
                log::info!("Auto-discovered DHCP Server Identifier: {}", ip);
                Ok(ip)
            }
            Err(e) => {
                log::warn!("Failed to auto-discover DHCP Server Identifier: {}.", e,);
                Err(anyhow!("failed to auto-discovery DHCP Server Identifier"))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_determine_server_identifier_with_valid_cli_arg() {
        // Test that a valid CLI-provided IP is used directly
        let cli_ip = "192.168.1.100".to_string();

        let result = determine_server_identifier(Some(&cli_ip));

        assert_eq!(
            result.unwrap(),
            "192.168.1.100".parse::<std::net::Ipv4Addr>().unwrap()
        );
    }

    #[test]
    fn test_determine_server_identifier_with_invalid_cli_arg() {
        // Test that an invalid CLI arg falls back to auto-discovery or gateway
        let cli_ip = "invalid-ip-address".to_string();

        let result = determine_server_identifier(Some(&cli_ip));

        // Result should be either auto-discovered IP or gateway, but not panic
        // We can't assert the exact value since it depends on network state,
        // but we can verify it returns a valid IPv4 address
        assert!(result.is_err());
    }

    #[test]
    fn test_determine_server_identifier_without_cli_arg() {
        // Test that when no CLI arg is provided, auto-discovery is attempted
        let result = determine_server_identifier(None).unwrap();

        // Should return either auto-discovered IP or gateway fallback
        // We can't assert the exact value since it depends on network state,
        // but we can verify it returns a valid IPv4 address
        assert_ne!(result, std::net::Ipv4Addr::UNSPECIFIED);
        assert_ne!(result, std::net::Ipv4Addr::LOCALHOST);
    }
}
