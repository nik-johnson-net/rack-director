use anyhow::Result;
use anyhow::anyhow;
use clap::Parser;
use clap::Subcommand;
use log::{error, info, warn};

use crate::console::start_console_command;

mod bmc;
mod console;
mod daemon;
mod partition;
mod scan;

#[derive(Subcommand, Debug)]
enum Command {
    /// Scans the device and uploads metadata to Rack Director.
    DeviceScan(scan::DeviceScanArgs),
    /// Configures the BMC with static IP and credentials.
    ConfigureBmc,
    /// Partitions disks according to the device's role disk layout.
    PartitionDisks,
    /// Continuously polls rack-director for actions and executes them without rebooting between actions.
    Daemon,
    /// Give a shell
    Console,
}

#[derive(Parser, Debug)]
struct Args {
    /// URL to the Rack Director API. Uses /proc/cmdline if not provided.
    #[arg(long, help = "URL to the Rack Director API")]
    director_url: Option<String>,

    /// Action to perform. Uses /proc/cmdline if not provided.
    #[arg(long, help = "Action to perform (device-scan)")]
    action: Option<String>,

    #[command(subcommand)]
    command: Option<Command>,
}

#[tokio::main]
async fn main() {
    // First step is to configure the logger.
    env_logger::init_from_env(env_logger::Env::default().filter_or("LOG", "info"));

    info!("Starting Rack Agent...");
    let args = Args::parse();
    let director_url = resolve_director_url(args.director_url)
        .await
        .unwrap_or_else(|e| {
            log::error!("Failed to resolve director URL: {e}");
            std::process::exit(1);
        });

    let client = common::cnc::CncClient::new(&director_url);

    // Determine which action to run: from CLI, from args.action, or from /proc/cmdline
    let result = if let Some(command) = args.command {
        // CLI subcommand takes precedence
        match command {
            Command::DeviceScan(device_args) => scan::device_scan(&client, &device_args).await,
            Command::ConfigureBmc => bmc::bmc_configure(&client).await,
            Command::PartitionDisks => partition::partition_disks(&client).await,
            Command::Daemon => daemon::run_daemon(&client).await,
            Command::Console => start_console_command(&client).await,
        }
    } else {
        // Read action from --action flag or /proc/cmdline
        let action = resolve_action(args.action).await.unwrap_or_else(|e| {
            warn!("Failed to resolve action: {e}, defaulting to device-scan");
            "device-scan".to_string()
        });

        info!("Running action: {}", action);
        match action.as_str() {
            "device-scan" => {
                let device_args = scan::DeviceScanArgs::new(false);
                scan::device_scan(&client, &device_args).await
            }
            "configure-bmc" => bmc::bmc_configure(&client).await,
            "partition-disks" => partition::partition_disks(&client).await,
            "daemon" => daemon::run_daemon(&client).await,
            "console" => start_console_command(&client).await,
            _ => {
                error!("Unknown action: {}", action);
                std::process::exit(1);
            }
        }
    };

    if let Err(e) = result {
        error!("{e}");
        std::process::exit(10);
    }
}

// Locate the director URL
async fn resolve_director_url(arg_director_url: Option<String>) -> Result<String> {
    if let Some(url) = arg_director_url {
        Ok(url)
    } else {
        // Fallback to /proc/cmdline
        let cmdline = tokio::fs::read_to_string("/proc/cmdline").await?;
        let url = cmdline
            .split_whitespace()
            .find(|s| s.starts_with("rackdirector.url="))
            .and_then(|s| s.strip_prefix("rackdirector.url="));
        Ok(url
            .ok_or(anyhow!("Failed to find rackdirector.url in /proc/cmdline. Try giving it as flag --director-url"))?
            .to_string())
    }
}

// Locate the action to perform
async fn resolve_action(arg_action: Option<String>) -> Result<String> {
    if let Some(action) = arg_action {
        Ok(action)
    } else {
        // Fallback to /proc/cmdline
        let cmdline = tokio::fs::read_to_string("/proc/cmdline").await?;
        let action = cmdline
            .split_whitespace()
            .find(|s| s.starts_with("rackdirector.action="))
            .and_then(|s| s.strip_prefix("rackdirector.action="));
        Ok(action
            .ok_or(anyhow!("Failed to find rackdirector.action in /proc/cmdline. Try giving it as flag --action"))?
            .to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_resolve_director_url_with_arg() {
        let result = resolve_director_url(Some("http://test:3000".to_string())).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "http://test:3000");
    }

    #[tokio::test]
    async fn test_resolve_action_with_arg() {
        let result = resolve_action(Some("device-scan".to_string())).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "device-scan");
    }

    #[test]
    fn test_device_scan_args_new() {
        let args = scan::DeviceScanArgs::new(true);
        assert!(args.no_upload);

        let args = scan::DeviceScanArgs::new(false);
        assert!(!args.no_upload);
    }
}
