mod agent;
mod boot;
mod config;
mod dhcp;
mod hardware_profiles;
mod http;
mod output;
mod server;
mod tftp;

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::net::Ipv4Addr;
use std::path::PathBuf;

use config::Config;
use output::Output;
use server::ServerState;

#[derive(Parser)]
#[command(name = "rack-simulator")]
#[command(about = "Simulate server behavior for testing Rack Director")]
struct Cli {
    /// Config file path
    #[arg(short, long, value_name = "FILE")]
    config: Option<PathBuf>,

    /// DHCP server port
    #[arg(long, default_value = "1067")]
    dhcp_port: u16,

    /// TFTP server port
    #[arg(long, default_value = "1069")]
    tftp_port: u16,

    /// HTTP server port
    #[arg(long, default_value = "3000")]
    http_port: u16,

    /// Server host
    #[arg(long, default_value = "127.0.0.1")]
    host: Ipv4Addr,

    /// Suppress step-by-step output
    #[arg(short, long)]
    quiet: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Full boot sequence (discovery + verify local boot)
    Boot {
        /// Server name from config
        server: String,
    },

    /// DHCP DISCOVER/OFFER only
    DhcpDiscover {
        /// Server name from config
        server: String,
    },

    /// DHCP REQUEST/ACK (requires prior discover)
    DhcpRequest {
        /// Server name from config
        server: String,
    },

    /// Download bootloader via TFTP
    TftpDownload {
        /// Server name from config
        server: String,
    },

    /// iPXE second-stage boot (DHCP + HTTP)
    IpxeBoot {
        /// Server name from config
        server: String,
    },

    /// Simulate rack-agent (update_attributes + action_success)
    AgentRun {
        /// Server name from config
        server: String,
    },

    /// Configuration management
    #[command(subcommand)]
    Config(ConfigCommands),
}

#[derive(Subcommand)]
enum ConfigCommands {
    /// Add a server to config
    CreateServer {
        /// Server name
        name: String,

        /// MAC address (or "auto" to generate)
        #[arg(long, default_value = "auto")]
        mac: String,

        /// UUID (or "auto" to generate)
        #[arg(long, default_value = "auto")]
        uuid: String,

        /// Architecture: x86-bios, x64-uefi, arm64-uefi
        #[arg(long, default_value = "x64-uefi")]
        arch: String,

        /// Hardware profile: dell-r640, dell-r750, hp-dl380, supermicro-x12, generic
        #[arg(long)]
        profile: Option<String>,
    },

    /// Remove a server from config
    RemoveServer {
        /// Server name
        name: String,
    },

    /// List configured servers
    List,

    /// Show server details
    Show {
        /// Server name
        name: String,
    },
}

struct ConnectionConfig {
    host: Ipv4Addr,
    dhcp_port: u16,
    tftp_port: u16,
    http_port: u16,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let output = Output::new(!cli.quiet);

    let config_path = cli.config.unwrap_or_else(|| {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("rack-simulator")
            .join("config.toml")
    });

    let conn = ConnectionConfig {
        host: cli.host,
        dhcp_port: cli.dhcp_port,
        tftp_port: cli.tftp_port,
        http_port: cli.http_port,
    };

    match cli.command {
        Commands::Boot { server } => {
            let config = Config::load(&config_path)?;
            let server_config = config.get_server(&server)?;
            boot::full_boot(&conn, &server_config, &output).await?;
        }

        Commands::DhcpDiscover { server } => {
            let config = Config::load(&config_path)?;
            let server_config = config.get_server(&server)?;
            let mut state = ServerState::load_or_create(&server, &server_config)?;
            // Use NIC 0 for individual command
            dhcp::discover(&conn, &mut state, 0, &output)?;
            state.save()?;
        }

        Commands::DhcpRequest { server } => {
            let config = Config::load(&config_path)?;
            let server_config = config.get_server(&server)?;
            let mut state = ServerState::load_or_create(&server, &server_config)?;
            // Use NIC 0 for individual command
            dhcp::request(&conn, &mut state, 0, &output)?;
            state.save()?;
        }

        Commands::TftpDownload { server } => {
            let config = Config::load(&config_path)?;
            let server_config = config.get_server(&server)?;
            let mut state = ServerState::load_or_create(&server, &server_config)?;
            tftp::download(&conn, &mut state, &output)?;
            state.save()?;
        }

        Commands::IpxeBoot { server } => {
            let config = Config::load(&config_path)?;
            let server_config = config.get_server(&server)?;
            let mut state = ServerState::load_or_create(&server, &server_config)?;
            boot::ipxe_boot(&conn, &mut state, &output).await?;
            state.save()?;
        }

        Commands::AgentRun { server } => {
            let config = Config::load(&config_path)?;
            let server_config = config.get_server(&server)?;
            let state = ServerState::load_or_create(&server, &server_config)?;
            agent::run(&conn, &state, &output).await?;
        }

        Commands::Config(config_cmd) => match config_cmd {
            ConfigCommands::CreateServer {
                name,
                mac,
                uuid,
                arch,
                profile,
            } => {
                config::create_server(&config_path, &name, &mac, &uuid, &arch, profile.as_deref())?;
                output.success(&format!("Created server '{}'", name));
            }

            ConfigCommands::RemoveServer { name } => {
                config::remove_server(&config_path, &name)?;
                output.success(&format!("Removed server '{}'", name));
            }

            ConfigCommands::List => {
                let config = Config::load(&config_path)?;
                config::list_servers(&config, &output);
            }

            ConfigCommands::Show { name } => {
                let config = Config::load(&config_path)?;
                config::show_server(&config, &name, &output)?;
            }
        },
    }

    Ok(())
}
