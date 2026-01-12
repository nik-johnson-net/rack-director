mod database;
mod dhcp;
mod director;
mod http;
mod lifecycle;
mod operating_systems;
mod plans;
mod roles;
mod storage;
mod templates;
mod tftp;

use std::{io, sync::Arc};

use anyhow::anyhow;
use clap::Parser;
use tokio::{sync::Mutex, task::JoinHandle};

use crate::director::Director;

const DEFAULT_DATABASE_PATH: &str = env!("RACK_DIRECTOR_DATABASE_PATH");

fn default_agent_images_path() -> String {
    format!("{}/agent-image", env!("RACK_DIRECTOR_INSTALL_PREFIX"))
}

#[derive(Parser, Debug)]
pub struct Args {
    // Path to the database file.
    #[arg(long, default_value_t = DEFAULT_DATABASE_PATH.to_string())]
    db_path: String,

    // Path to the directory containing the TFTP files.
    #[arg(long, default_value = "/usr/lib/rack-director/tftp")]
    tftp_path: String,

    // DHCP server address (optional, defaults to 67)
    #[arg(long, default_value = "0.0.0.0:67")]
    dhcp_address: String,

    // HTTP server address
    #[arg(long, default_value = "0.0.0.0:3000")]
    http_address: String,

    // TFTP server address
    #[arg(long, default_value = "0.0.0.0:69")]
    tftp_address: String,

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
        default_value = "local",
        help = "Image storage type: local or s3"
    )]
    storage_type: String,

    #[arg(
        long,
        default_value = "/var/lib/rack-director/images",
        help = "Local storage path (when storage-type=local)"
    )]
    storage_path: String,

    #[arg(long, help = "S3 endpoint URL (when storage-type=s3)")]
    s3_endpoint: Option<String>,

    #[arg(long, help = "S3 bucket name (when storage-type=s3)")]
    s3_bucket: Option<String>,

    #[arg(
        long,
        default_value = "us-east-1",
        help = "S3 region (when storage-type=s3)"
    )]
    s3_region: String,

    #[arg(long, help = "Base URL for serving images over HTTP")]
    storage_base_url: Option<String>,

    // Agent images path (bundled with installation)
    #[arg(
        long,
        default_value_t = default_agent_images_path(),
        help = "Path to agent image files (vmlinuz, initramfs.img)"
    )]
    agent_images_path: String,
}

pub struct RackDirectorHandle {
    // Information for the http service
    http_handle: JoinHandle<Result<(), io::Error>>,
    pub http_port: u16,

    // Information for the tftp service
    tftp_handle: JoinHandle<Result<(), anyhow::Error>>,
    pub tftp_port: u16,

    // Information for the dhcp service
    dhcp_handle: JoinHandle<Result<(), anyhow::Error>>,
    pub dhcp_port: u16,
}

impl RackDirectorHandle {
    // Wait for the services to stop.
    // TODO: Currently just waiting for any one to abort. Need a proper abort signal / shutdown architecture.
    pub async fn wait(self) {
        let _ = tokio::try_join!(self.http_handle, self.tftp_handle, self.dhcp_handle);
    }
}

pub async fn rack_director_start(args: crate::Args) -> Result<RackDirectorHandle, anyhow::Error> {
    // Initialize database connection
    let db_file = format!("{}/db.sqlite", args.db_path);
    let db = Arc::new(Mutex::new(database::open(&db_file).unwrap()));

    // Initialize individual stores
    let os_store = operating_systems::OperatingSystemsStore::new(db.clone());
    let roles_store = roles::RolesStore::new(db.clone());

    // Initialize storage
    let storage_config = build_storage_config(&args)?;
    let image_store = storage::create_image_store(storage_config).await?;

    // Initialize Director
    let public_url = args.http_public_url.to_owned().unwrap_or_default();
    let director: Director = Director::new(db.clone(), image_store.clone(), &public_url);

    // Initialize DHCP server and store
    let dhcp_store = dhcp::DhcpStore::new(db.clone());

    // Determine TFTP public address - use explicit arg if provided, otherwise use gateway from config
    let dhcp_config = dhcp_store.load_config().await.unwrap();
    let tftp_public = args
        .tftp_public_address
        .clone()
        .unwrap_or_else(|| dhcp_config.gateway.clone());

    let http_server = public_url.clone();

    // Determine DHCP Server Identifier (Option 54)
    // Priority: CLI arg > auto-discovered IP > fallback to gateway
    let server_identifier = determine_server_identifier(args.dhcp_server_identifier.as_ref())?;

    let dhcp_server = dhcp::DhcpServer::new(
        db.clone(),
        director.clone(),
        tftp_public,
        http_server,
        server_identifier,
        Some(args.dhcp_address.clone()),
    )
    .await
    .unwrap();

    // Initialize TFTP Handler
    let tftp_handler = director::DirectorTftpHandler::new(args.tftp_path.clone());
    let mut tftp_server = tftp::Server::new(tftp_handler);
    tftp_server.address(args.tftp_address);

    // Start HTTP Service
    let http_start_result = http::start(
        director.clone(),
        dhcp_store,
        image_store,
        os_store,
        roles_store,
        args.http_address,
        args.agent_images_path,
    )
    .await?;

    // Start TFTP Service
    let tftp_start_result = tftp_server.serve().await?;

    // Start DHCP Service
    let dhcp_start_result = dhcp_server.serve().await?;

    Ok(RackDirectorHandle {
        http_handle: http_start_result.join_handle,
        http_port: http_start_result.port,

        tftp_handle: tftp_start_result.join_handle,
        tftp_port: tftp_start_result.port,

        dhcp_handle: dhcp_start_result.join_handle,
        dhcp_port: dhcp_start_result.port,
    })
}

fn build_storage_config(args: &Args) -> Result<storage::ImageStoreConfig, anyhow::Error> {
    let base_url = args
        .storage_base_url
        .clone()
        .unwrap_or_else(|| format!("http://{}/images", args.http_address));

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
