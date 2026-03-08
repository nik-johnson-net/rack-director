mod agent;
mod boot;
mod config;
mod dhcp;
mod e2e;
mod hardware_profiles;
mod http;
mod output;
mod server;
mod tftp;
mod vm;

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
enum E2eCommand {
    /// Run a single e2e test
    Run {
        /// Path to test TOML file
        test_file: PathBuf,
        #[arg(long, default_value = ".local-storage/agent-image/vmlinuz")]
        agent_kernel: PathBuf,
        #[arg(long, default_value = ".local-storage/agent-image/initramfs.img")]
        agent_initramfs: PathBuf,
        #[arg(long, default_value = ".local-storage/director-image/vmlinuz-director")]
        director_kernel: PathBuf,
        #[arg(
            long,
            default_value = ".local-storage/director-image/director-initramfs.img"
        )]
        director_initramfs: PathBuf,
        #[arg(
            long,
            default_value = ".local-storage/installer-cache/rocky-10.1-vmlinuz"
        )]
        rocky_installer_kernel: PathBuf,
        #[arg(
            long,
            default_value = ".local-storage/installer-cache/rocky-10.1-initrd.img"
        )]
        rocky_installer_initramfs: PathBuf,
        #[arg(long, default_value = "e2e-tests/rocky-linux-10.1-ks.cfg")]
        rocky_installer_kickstart: PathBuf,
        #[arg(long)]
        serial_logs_dir: Option<PathBuf>,
    },
    /// Run all e2e tests in a directory
    RunAll {
        /// Directory containing test TOML files
        tests_dir: PathBuf,
        /// Run tests in parallel
        #[arg(long)]
        parallel: bool,
        #[arg(long, default_value = ".local-storage/agent-image/vmlinuz")]
        agent_kernel: PathBuf,
        #[arg(long, default_value = ".local-storage/agent-image/initramfs.img")]
        agent_initramfs: PathBuf,
        #[arg(long, default_value = ".local-storage/director-image/vmlinuz-director")]
        director_kernel: PathBuf,
        #[arg(
            long,
            default_value = ".local-storage/director-image/director-initramfs.img"
        )]
        director_initramfs: PathBuf,
        #[arg(
            long,
            default_value = ".local-storage/installer-cache/rocky-10.1-vmlinuz"
        )]
        rocky_installer_kernel: PathBuf,
        #[arg(
            long,
            default_value = ".local-storage/installer-cache/rocky-10.1-initrd.img"
        )]
        rocky_installer_initramfs: PathBuf,
        #[arg(long, default_value = "e2e-tests/rocky-linux-10.1-ks.cfg")]
        rocky_installer_kickstart: PathBuf,
        #[arg(long)]
        serial_logs_dir: Option<PathBuf>,
    },
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

    /// Run QEMU-based end-to-end integration tests
    E2e {
        #[command(subcommand)]
        command: E2eCommand,
    },
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

        /// Architecture: x86-bios, x64-uefi, arm64-uefi, x64-uefi-http
        #[arg(long, default_value = "x64-uefi")]
        arch: String,

        /// Hardware profile: dell-r640, dell-r750, hp-dl380, supermicro-x12, generic
        #[arg(long)]
        profile: Option<String>,

        /// BMC MAC address (or "auto" to generate)
        #[arg(long)]
        bmc_mac: Option<String>,

        /// BMC Source (DHCP or Static)
        #[arg(long, default_value = "DHCP")]
        bmc_source: String,

        /// Required if BMC Source is Static
        #[arg(long)]
        bmc_ip_address: Option<Ipv4Addr>,

        /// Required if BMC Source is Static
        #[arg(long)]
        bmc_netmask: Option<Ipv4Addr>,

        /// Required if BMC Source is Static
        #[arg(long)]
        bmc_gateway: Option<Ipv4Addr>,
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
            dhcp::discover(
                &conn,
                &mut state,
                dhcp::DiscoverType::Nic { index: 0 },
                &output,
            )?;
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

        Commands::E2e { command } => match command {
            E2eCommand::Run {
                test_file,
                agent_kernel,
                agent_initramfs,
                director_kernel,
                director_initramfs,
                rocky_installer_kernel,
                rocky_installer_initramfs,
                rocky_installer_kickstart,
                serial_logs_dir,
            } => {
                let run_config = e2e::runner::TestRunConfig {
                    agent_kernel,
                    agent_initramfs,
                    director_kernel,
                    director_initramfs,
                    rocky_installer_kernel,
                    rocky_installer_initramfs,
                    rocky_installer_kickstart,
                    serial_logs_dir,
                };
                let result = e2e::runner::run_test(&test_file, &run_config, &output).await?;
                if !result.passed {
                    std::process::exit(1);
                }
            }
            E2eCommand::RunAll {
                tests_dir,
                parallel,
                agent_kernel,
                agent_initramfs,
                director_kernel,
                director_initramfs,
                rocky_installer_kernel,
                rocky_installer_initramfs,
                rocky_installer_kickstart,
                serial_logs_dir,
            } => {
                let run_config = e2e::runner::TestRunConfig {
                    agent_kernel,
                    agent_initramfs,
                    director_kernel,
                    director_initramfs,
                    rocky_installer_kernel,
                    rocky_installer_initramfs,
                    rocky_installer_kickstart,
                    serial_logs_dir,
                };
                let results = if parallel {
                    e2e::runner::run_all_parallel(&tests_dir, &run_config, &output).await?
                } else {
                    e2e::runner::run_all(&tests_dir, &run_config, &output).await?
                };
                let failed_count = results.iter().filter(|r| !r.passed).count();
                if failed_count > 0 {
                    output.error(&format!("{} test(s) failed", failed_count));
                    std::process::exit(1);
                }
                output.success(&format!("All {} test(s) passed", results.len()));
            }
        },

        Commands::Config(config_cmd) => match config_cmd {
            ConfigCommands::CreateServer {
                name,
                mac,
                uuid,
                arch,
                profile,
                bmc_mac,
                bmc_source,
                bmc_ip_address,
                bmc_netmask,
                bmc_gateway,
            } => {
                config::create_server(
                    &config_path,
                    &name,
                    &mac,
                    &uuid,
                    &arch,
                    profile.as_deref(),
                    bmc_mac.as_deref(),
                    &bmc_source,
                    bmc_ip_address.as_ref(),
                    bmc_netmask.as_ref(),
                    bmc_gateway.as_ref(),
                )?;
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
